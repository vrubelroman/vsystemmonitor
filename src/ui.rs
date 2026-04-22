use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Gauge, Paragraph, Wrap},
    Frame,
};

use crate::{
    app::App,
    model::{DiskInfo, HostInfo, HostStatus, HostType},
    theme::{palette, Palette},
};

pub fn render(frame: &mut Frame, app: &App) {
    let palette = palette(&app.config);
    frame.render_widget(Block::default().style(Style::default().bg(palette.base)), frame.size());

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(if app.config.show_help_hints { 1 } else { 0 }),
        ])
        .split(frame.size());

    render_header(frame, layout[0], app, palette);
    render_hosts(frame, layout[1], app, palette);

    if app.config.show_help_hints {
        render_footer(frame, layout[2], app, palette);
    }

    if app.show_help {
        render_help_overlay(frame, app, palette);
    }
}

fn render_header(frame: &mut Frame, area: Rect, app: &App, palette: Palette) {
    let page = app.pager.current_page() + 1;
    let total_pages = app.pager.total_pages(app.hosts.len()).max(1);
    let title = Line::from(vec![
        Span::styled("vsysmonitor", Style::default().fg(palette.mauve).add_modifier(Modifier::BOLD)),
        Span::raw("  "),
        Span::styled("theme:", Style::default().fg(palette.overlay)),
        Span::styled(format!(" {}", app.config.theme.as_str()), Style::default().fg(palette.text)),
        Span::raw("  "),
        Span::styled("page:", Style::default().fg(palette.overlay)),
        Span::styled(format!(" {page}/{total_pages}"), Style::default().fg(palette.text)),
    ]);

    frame.render_widget(Paragraph::new(title).alignment(Alignment::Left), area);
}

fn render_hosts(frame: &mut Frame, area: Rect, app: &App, palette: Palette) {
    let (start, end) = app.pager.window(app.hosts.len());
    let visible_hosts = &app.hosts[start..end];
    if visible_hosts.is_empty() {
        let empty = Paragraph::new("No hosts configured").style(Style::default().fg(palette.subtext));
        let block = centered_block("Hosts", app.config.show_borders, palette);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        frame.render_widget(empty, inner);
        return;
    }

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(vec![Constraint::Ratio(1, visible_hosts.len() as u32); visible_hosts.len()])
        .split(area);

    for (host, chunk) in visible_hosts.iter().zip(columns.iter().copied()) {
        render_host_column(frame, chunk, host, app, palette);
    }
}

fn render_host_column(frame: &mut Frame, area: Rect, host: &HostInfo, app: &App, palette: Palette) {
    let border_color = match host.status {
        HostStatus::Online => palette.sapphire,
        HostStatus::Loading => palette.yellow,
        HostStatus::Unreachable | HostStatus::Error => palette.red,
    };

    let title = format!(
        "{} | {} | {}",
        host.display_name,
        host_type_label(host.host_type),
        host_status_label(host.status)
    );
    let outer = Block::default()
        .title(title)
        .borders(if app.config.show_borders {
            Borders::ALL
        } else {
            Borders::NONE
        })
        .style(Style::default().bg(palette.mantle).fg(palette.text))
        .border_style(Style::default().fg(border_color));
    frame.render_widget(outer.clone(), area);

    let inner = outer.inner(area);
    let disk_lines = host.metrics.disks.len().max(2) as u16;
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(disk_lines),
            Constraint::Length(2),
        ])
        .split(inner);

    let cpu_color = palette.severity_color(
        cpu_gauge_severity_value(host, app),
        app.config.cpu_warning_threshold,
        app.config.cpu_critical_threshold,
    );
    render_gauge(
        frame,
        layout[0],
        "CPU",
        host.metrics.cpu_usage_percent,
        cpu_gauge_label(host),
        cpu_color,
        palette,
    );

    let ram_color = palette.severity_color(
        host.metrics.memory_usage_percent,
        app.config.ram_warning_threshold,
        app.config.ram_critical_threshold,
    );
    render_gauge(
        frame,
        layout[1],
        "RAM",
        host.metrics.memory_usage_percent,
        format!(
            "{} / {}",
            format_bytes(host.metrics.memory_used_bytes),
            format_bytes(host.metrics.memory_total_bytes)
        ),
        ram_color,
        palette,
    );

    render_disks(frame, layout[2], &host.metrics.disks, app, palette);
    render_status(frame, layout[3], host, app, palette);
}

fn render_gauge(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    value: f64,
    label: String,
    color: ratatui::style::Color,
    palette: Palette,
) {
    let gauge = Gauge::default()
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(palette.overlay))
                .style(Style::default().bg(palette.crust)),
        )
        .gauge_style(Style::default().fg(color).bg(palette.crust).add_modifier(Modifier::BOLD))
        .label(label)
        .percent(value.clamp(0.0, 100.0).round() as u16);
    frame.render_widget(gauge, area);
}

fn cpu_gauge_label(host: &HostInfo) -> String {
    match host.metrics.cpu_temperature_celsius {
        Some(temp) => format!("{:.1}% | {:.1}C", host.metrics.cpu_usage_percent, temp),
        None => format!("{:.1}%", host.metrics.cpu_usage_percent),
    }
}

fn cpu_gauge_severity_value(host: &HostInfo, app: &App) -> f64 {
    let usage_severity = severity_rank(
        host.metrics.cpu_usage_percent,
        app.config.cpu_warning_threshold,
        app.config.cpu_critical_threshold,
    );
    let temp_severity = host
        .metrics
        .cpu_temperature_celsius
        .map(|temp| {
            severity_rank(
                temp,
                app.config.cpu_temp_warning_threshold,
                app.config.cpu_temp_critical_threshold,
            )
        })
        .unwrap_or(0);

    match usage_severity.max(temp_severity) {
        2 => app.config.cpu_critical_threshold,
        1 => app.config.cpu_warning_threshold,
        _ => 0.0,
    }
}

fn severity_rank(value: f64, warning: f64, critical: f64) -> u8 {
    if value >= critical {
        2
    } else if value >= warning {
        1
    } else {
        0
    }
}

fn render_disks(frame: &mut Frame, area: Rect, disks: &[DiskInfo], app: &App, palette: Palette) {
    let lines = if disks.is_empty() {
        vec![Line::from(Span::styled(
            "No disks matched current filters",
            Style::default().fg(palette.subtext),
        ))]
    } else {
        disks.iter()
            .map(|disk| {
                let color = palette.severity_color(
                    disk.usage_percent,
                    app.config.disk_warning_threshold,
                    app.config.disk_critical_threshold,
                );
                Line::from(vec![
                    Span::styled(
                        format!("{:<10}", disk_label(disk)),
                        Style::default().fg(palette.sapphire).add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" "),
                    Span::styled(
                        format!(
                            "{:>5.1}%  {} / {}{}",
                            disk.usage_percent,
                            format_bytes(disk.used_bytes),
                            format_bytes(disk.total_bytes),
                            disk_mount_suffix(disk),
                        ),
                        Style::default().fg(color),
                    ),
                ])
            })
            .collect()
    };

    let disks_widget = Paragraph::new(lines)
        .block(
            Block::default()
                .title("Disks")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(palette.overlay))
                .style(Style::default().bg(palette.crust)),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(disks_widget, area);
}

fn render_status(frame: &mut Frame, area: Rect, host: &HostInfo, app: &App, palette: Palette) {
    let stale = host
        .last_updated
        .and_then(|ts| SystemTime::now().duration_since(ts).ok())
        .map(|elapsed| elapsed > Duration::from_millis(app.config.stale_data_timeout_ms))
        .unwrap_or(true);

    let mut spans = vec![
        Span::styled("updated ", Style::default().fg(palette.overlay)),
        Span::styled(
            host.last_updated
                .map(format_timestamp)
                .unwrap_or_else(|| "never".to_string()),
            Style::default().fg(palette.stale_color(stale)),
        ),
    ];

    if let Some(error) = &host.last_error {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(error, Style::default().fg(palette.red)));
    }

    let status = Paragraph::new(Line::from(spans))
        .block(Block::default().title("Status").borders(Borders::ALL).border_style(
            Style::default().fg(palette.overlay),
        ))
        .wrap(Wrap { trim: true });
    frame.render_widget(status, area);
}

fn render_footer(frame: &mut Frame, area: Rect, app: &App, palette: Palette) {
    let hints = format!(
        "{} prev  {} next  {} refresh  {} help  {} quit",
        app.config.keys.prev_page,
        app.config.keys.next_page,
        app.config.keys.refresh,
        app.config.keys.help,
        app.config.keys.quit
    );
    let widget = Paragraph::new(hints).style(Style::default().fg(palette.subtext));
    frame.render_widget(widget, area);
}

fn render_help_overlay(frame: &mut Frame, app: &App, palette: Palette) {
    let area = centered_rect(60, 40, frame.size());
    frame.render_widget(Clear, area);
    let lines = vec![
        Line::from(vec![
            Span::styled("Navigation", Style::default().fg(palette.mauve).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(format!("{} / {}  switch pages", app.config.keys.prev_page, app.config.keys.next_page)),
        Line::from(format!("{}  refresh metrics", app.config.keys.refresh)),
        Line::from(format!("{}  toggle help", app.config.keys.help)),
        Line::from(format!("{}  quit application", app.config.keys.quit)),
        Line::from(""),
        Line::from("Prepared for stage 2: the UI already paginates over a host list."),
    ];
    let help = Paragraph::new(lines)
        .alignment(Alignment::Left)
        .wrap(Wrap { trim: true })
        .block(
            Block::default()
                .title("Help")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(palette.blue))
                .style(Style::default().bg(palette.mantle).fg(palette.text)),
        );
    frame.render_widget(help, area);
}

fn centered_rect(width_percent: u16, height_percent: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - height_percent) / 2),
            Constraint::Percentage(height_percent),
            Constraint::Percentage((100 - height_percent) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - width_percent) / 2),
            Constraint::Percentage(width_percent),
            Constraint::Percentage((100 - width_percent) / 2),
        ])
        .split(vertical[1])[1]
}

fn centered_block<'a>(title: &'a str, show_borders: bool, palette: Palette) -> Block<'a> {
    Block::default()
        .title(title)
        .borders(if show_borders { Borders::ALL } else { Borders::NONE })
        .style(Style::default().bg(palette.mantle).fg(palette.text))
}

fn host_type_label(host_type: HostType) -> &'static str {
    match host_type {
        HostType::Local => "local",
        HostType::Remote => "remote",
    }
}

fn host_status_label(status: HostStatus) -> &'static str {
    match status {
        HostStatus::Online => "online",
        HostStatus::Loading => "loading",
        HostStatus::Unreachable => "unreachable",
        HostStatus::Error => "error",
    }
}

fn disk_label(disk: &DiskInfo) -> String {
    if disk.name.is_empty() {
        disk.mount_point.clone()
    } else {
        disk.name.clone()
    }
}

fn disk_mount_suffix(disk: &DiskInfo) -> String {
    if disk.mount_point.is_empty() {
        return String::new();
    }

    let mountpoints = disk
        .mount_point
        .split(',')
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();

    if mountpoints.is_empty() {
        return String::new();
    }

    if mountpoints.len() == 1 {
        return format!("  {}", mountpoints[0]);
    }

    if mountpoints.len() == 2 {
        return format!("  {},{}", mountpoints[0], mountpoints[1]);
    }

    if let Some(root_mount) = mountpoints.iter().find(|mountpoint| **mountpoint == "/") {
        return format!("  {} +{}", root_mount, mountpoints.len() - 1);
    }

    format!("  {},{} +{}", mountpoints[0], mountpoints[1], mountpoints.len() - 2)
}

#[cfg(test)]
mod ui_tests {
    use super::disk_mount_suffix;
    use crate::model::DiskInfo;

    #[test]
    fn compresses_many_mountpoints_in_suffix() {
        let disk = DiskInfo {
            name: "nvme1n1".to_string(),
            mount_point: "/boot,/,/tmp,/nix/store".to_string(),
            used_bytes: 0,
            total_bytes: 1,
            usage_percent: 0.0,
        };

        assert_eq!(disk_mount_suffix(&disk), "  / +3");
    }
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut idx = 0usize;
    while value >= 1024.0 && idx < UNITS.len() - 1 {
        value /= 1024.0;
        idx += 1;
    }
    format!("{value:.1}{}", UNITS[idx])
}

fn format_timestamp(timestamp: SystemTime) -> String {
    match timestamp.duration_since(UNIX_EPOCH) {
        Ok(duration) => {
            let total = duration.as_secs() % 86_400;
            let hours = total / 3_600;
            let minutes = (total % 3_600) / 60;
            let seconds = total % 60;
            format!("{hours:02}:{minutes:02}:{seconds:02}")
        }
        Err(_) => "invalid".to_string(),
    }
}
