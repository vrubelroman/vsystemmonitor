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
    model::{DiskInfo, DockerContainerInfo, HostInfo, HostStatus, HostType},
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
    let selected = app.selected_host_index() + 1;
    let total_hosts = app.hosts.len().max(1);
    let title = Line::from(vec![
        Span::styled("vsysmonitor", Style::default().fg(palette.mauve).add_modifier(Modifier::BOLD)),
        Span::raw("  "),
        Span::styled("theme:", Style::default().fg(palette.overlay)),
        Span::styled(format!(" {}", app.config.theme.as_str()), Style::default().fg(palette.text)),
        Span::raw("  "),
        Span::styled("host:", Style::default().fg(palette.overlay)),
        Span::styled(format!(" {selected}/{total_hosts}"), Style::default().fg(palette.text)),
    ]);

    frame.render_widget(Paragraph::new(title).alignment(Alignment::Left), area);
}

fn render_hosts(frame: &mut Frame, area: Rect, app: &App, palette: Palette) {
    if app.hosts.is_empty() {
        let empty = Paragraph::new("No hosts configured").style(Style::default().fg(palette.subtext));
        let block = centered_block("Hosts", app.config.show_borders, palette);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        frame.render_widget(empty, inner);
        return;
    }

    let layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(34), Constraint::Min(0)])
        .split(area);

    render_host_list(frame, layout[0], app, palette);

    if let Some(host) = app.selected_host() {
        render_host_column(frame, layout[1], host, app, palette);
    }
}

fn render_host_list(frame: &mut Frame, area: Rect, app: &App, palette: Palette) {
    let block = Block::default()
        .title("Hosts")
        .borders(if app.config.show_borders {
            Borders::ALL
        } else {
            Borders::NONE
        })
        .style(Style::default().bg(palette.mantle).fg(palette.text))
        .border_style(Style::default().fg(palette.overlay));
    frame.render_widget(block.clone(), area);

    let inner = block.inner(area);
    let visible_items = (inner.height as usize).max(4) / 4;
    let selected_index = app.selected_host_index();
    let start = selected_index.saturating_sub(visible_items / 2);
    let end = (start + visible_items).min(app.hosts.len());
    let start = end.saturating_sub(visible_items);

    let mut lines = Vec::new();
    for (index, host) in app.hosts[start..end].iter().enumerate() {
        let actual_index = start + index;
        let selected = actual_index == selected_index;
        lines.extend(host_list_item_lines(host, selected, inner.width as usize, app, palette));
    }

    let widget = Paragraph::new(lines)
        .style(Style::default().bg(palette.mantle).fg(palette.text))
        .wrap(Wrap { trim: true });
    frame.render_widget(widget, inner);
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
    let disk_lines = disk_widget_height(host);
    let docker_lines = docker_widget_lines(host);
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(disk_lines),
            Constraint::Min(docker_lines),
            Constraint::Length(3),
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
    render_docker(frame, layout[3], host, palette);
    render_status(frame, layout[4], host, app, palette);
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

fn render_docker(frame: &mut Frame, area: Rect, host: &HostInfo, palette: Palette) {
    let lines = if let Some(error) = &host.metrics.docker_error {
        vec![Line::from(Span::styled(
            error,
            Style::default().fg(palette.red),
        ))]
    } else if host.metrics.docker_containers.is_empty() {
        vec![Line::from(Span::styled(
            "No running containers",
            Style::default().fg(palette.subtext),
        ))]
    } else {
        let inner_width = area.width.saturating_sub(2) as usize;
        let (image_width, created_width, status_width) = docker_column_widths(inner_width);

        let mut lines = vec![docker_header_line(
            image_width,
            created_width,
            status_width,
            palette,
        )];
        lines.extend(
            host.metrics
                .docker_containers
                .iter()
                .map(|container| {
                    docker_container_line(
                        container,
                        image_width,
                        created_width,
                        status_width,
                        palette,
                    )
                }),
        );
        lines
    };

    let docker_widget = Paragraph::new(lines)
        .block(
            Block::default()
                .title("Docker")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(palette.overlay))
                .style(Style::default().bg(palette.crust)),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(docker_widget, area);
}

fn render_footer(frame: &mut Frame, area: Rect, app: &App, palette: Palette) {
    let hints = format!(
        "{} / Up / Left prev  {} / Down / Right next  {} refresh  {} help  {} quit",
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
        Line::from(format!("{} / Up / Left  previous host", app.config.keys.prev_page)),
        Line::from(format!(
            "{} / Down / Right  next host",
            app.config.keys.next_page
        )),
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

fn docker_widget_lines(host: &HostInfo) -> u16 {
    if host.metrics.docker_error.is_some() || host.metrics.docker_containers.is_empty() {
        3
    } else {
        host.metrics.docker_containers.len() as u16 + 3
    }
}

fn disk_widget_height(host: &HostInfo) -> u16 {
    let content_lines = if host.metrics.disks.is_empty() {
        1
    } else {
        host.metrics.disks.len()
    };
    content_lines as u16 + 2
}

fn docker_header_line(
    image_width: usize,
    created_width: usize,
    status_width: usize,
    palette: Palette,
) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            pad_or_truncate("IMAGE", image_width),
            Style::default().fg(palette.overlay).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            pad_or_truncate("CREATED", created_width),
            Style::default().fg(palette.overlay).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            pad_or_truncate("STATUS", status_width),
            Style::default().fg(palette.overlay).add_modifier(Modifier::BOLD),
        ),
    ])
}

fn docker_container_line(
    container: &DockerContainerInfo,
    image_width: usize,
    created_width: usize,
    status_width: usize,
    palette: Palette,
) -> Line<'static> {
    let image = truncate_text(&fallback_text(&container.image, "-"), 35);

    Line::from(vec![
        Span::styled(
            pad_or_truncate(&image, image_width),
            Style::default().fg(palette.sapphire),
        ),
        Span::raw(" "),
        Span::styled(
            pad_or_truncate(&fallback_text(&container.created, "-"), created_width),
            Style::default().fg(palette.subtext),
        ),
        Span::raw(" "),
        Span::styled(
            pad_or_truncate(&fallback_text(&container.status, "-"), status_width),
            Style::default().fg(docker_status_color(container, palette)),
        ),
    ])
}

fn docker_container_is_problem(container: &DockerContainerInfo) -> bool {
    let status = container.status.to_ascii_lowercase();
    status.contains("restarting")
        || status.contains("unhealthy")
        || status.contains("dead")
        || status.contains("paused")
        || status.contains("removing")
}

fn docker_status_color(container: &DockerContainerInfo, palette: Palette) -> ratatui::style::Color {
    let status = container.status.to_ascii_lowercase();
    if docker_container_is_problem(container) {
        palette.red
    } else if status.contains("healthy") || status.starts_with("up ") || status == "up" {
        palette.green
    } else {
        palette.yellow
    }
}

fn docker_column_widths(inner_width: usize) -> (usize, usize, usize) {
    let content_width = inner_width.saturating_sub(2);
    let total_width = content_width.saturating_sub(2);

    if total_width <= 24 {
        return (10, 6, total_width.saturating_sub(16).max(8));
    }

    let image_width = (total_width * 48 / 100).clamp(16, 36);
    let created_width = (total_width * 18 / 100).clamp(10, 14);
    let status_width = total_width.saturating_sub(image_width + created_width).max(12);

    (image_width, created_width, status_width)
}

fn fallback_text(value: &str, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value.trim().to_string()
    }
}

fn truncate_text(value: &str, max_chars: usize) -> String {
    let chars = value.chars().collect::<Vec<_>>();
    if chars.len() <= max_chars {
        return value.to_string();
    }

    if max_chars <= 3 {
        return ".".repeat(max_chars);
    }

    chars[..max_chars - 3].iter().collect::<String>() + "..."
}

fn pad_or_truncate(value: &str, width: usize) -> String {
    let value = truncate_text(value, width);
    format!("{value:<width$}")
}

fn host_list_item_lines(
    host: &HostInfo,
    selected: bool,
    width: usize,
    app: &App,
    palette: Palette,
) -> [Line<'static>; 4] {
    let border_color = if selected { palette.green } else { palette.overlay };
    let border_style = Style::default().fg(border_color).bg(palette.mantle);
    let name_style = Style::default()
        .fg(host_list_name_color(host, app, palette))
        .bg(palette.mantle)
        .add_modifier(Modifier::BOLD);
    let detail_style = Style::default()
        .fg(host_list_detail_color(host, palette))
        .bg(palette.mantle);

    let inner_width = width.saturating_sub(2);
    let name = truncate_text(&host.display_name, 28);
    let summary = match host.status {
        HostStatus::Loading => "waiting for metrics...".to_string(),
        HostStatus::Unreachable | HostStatus::Error => {
            fallback_text(host.last_error.as_deref().unwrap_or(host_status_label(host.status)), "-")
        }
        HostStatus::Online => {
            let temp = host
                .metrics
                .cpu_temperature_celsius
                .map(|value| format!(" {}C", value.round() as i64))
                .unwrap_or_default();
            format!(
                "CPU {:>4.1}%{}  RAM {:>4.1}%",
                host.metrics.cpu_usage_percent,
                temp,
                host.metrics.memory_usage_percent
            )
        }
    };
    let (top_left, top_right, bottom_left, bottom_right, horizontal, vertical) = if selected {
        ('┏', '┓', '┗', '┛', '━', '┃')
    } else {
        ('┌', '┐', '└', '┘', '─', '│')
    };
    let top_border = format!("{top_left}{}{top_right}", horizontal.to_string().repeat(inner_width));
    let bottom_border =
        format!("{bottom_left}{}{bottom_right}", horizontal.to_string().repeat(inner_width));

    [
        Line::from(Span::styled(top_border, border_style)),
        Line::from(vec![
            Span::styled(vertical.to_string(), border_style),
            Span::styled(pad_or_truncate(&format!(" {name}"), inner_width), name_style),
            Span::styled(vertical.to_string(), border_style),
        ]),
        Line::from(vec![
            Span::styled(vertical.to_string(), border_style),
            Span::styled(
                pad_or_truncate(
                    &format!(" {}", truncate_text(&summary, inner_width.saturating_sub(1))),
                    inner_width,
                ),
                detail_style,
            ),
            Span::styled(vertical.to_string(), border_style),
        ]),
        Line::from(Span::styled(bottom_border, border_style)),
    ]
}

fn host_list_name_color(host: &HostInfo, app: &App, palette: Palette) -> ratatui::style::Color {
    match host.status {
        HostStatus::Unreachable | HostStatus::Error => return palette.red,
        HostStatus::Loading => return palette.yellow,
        HostStatus::Online => {}
    }

    if host_has_problem_docker(host) {
        return palette.red;
    }

    if let Some(temp) = host.metrics.cpu_temperature_celsius {
        return palette.severity_color(
            temp,
            app.config.cpu_temp_warning_threshold,
            app.config.cpu_temp_critical_threshold,
        );
    }

    palette.green
}

fn host_list_detail_color(host: &HostInfo, palette: Palette) -> ratatui::style::Color {
    match host.status {
        HostStatus::Unreachable | HostStatus::Error => palette.red,
        HostStatus::Loading => palette.yellow,
        HostStatus::Online => {
            if host_has_problem_docker(host) {
                palette.red
            } else {
                palette.subtext
            }
        }
    }
}

fn host_has_problem_docker(host: &HostInfo) -> bool {
    host.metrics.docker_error.is_some()
        || host
            .metrics
            .docker_containers
            .iter()
            .any(docker_container_is_problem)
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
