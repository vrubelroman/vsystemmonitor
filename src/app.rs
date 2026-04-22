use std::{
    io::{self, Stdout},
    time::{Duration, Instant},
};

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::{
    collector::{local::LocalCollector, remote::load_remote_collectors, HostCollector},
    config::AppConfig,
    model::{HostDescriptor, HostInfo, HostStatus, HostType},
    navigation::Pager,
    ui,
};

pub struct App {
    pub config: AppConfig,
    pub hosts: Vec<HostInfo>,
    pub pager: Pager,
    pub show_help: bool,
    should_quit: bool,
    collectors: Vec<CollectorState>,
}

struct CollectorState {
    descriptor: HostDescriptor,
    collector: Box<dyn HostCollector>,
    host: HostInfo,
    last_refresh_at: Option<Instant>,
}

impl App {
    pub fn new(config: AppConfig) -> Self {
        let mut collector_impls: Vec<Box<dyn HostCollector>> = vec![Box::new(LocalCollector::new(&config))];
        if let Ok(remote_collectors) = load_remote_collectors(&config) {
            collector_impls.extend(
                remote_collectors
                    .into_iter()
                    .map(|collector| Box::new(collector) as Box<dyn HostCollector>),
            );
        }
        let collectors = collector_impls
            .into_iter()
            .map(|collector| {
                let descriptor = collector.descriptor();
                let host = HostInfo::loading(descriptor.clone());
                CollectorState {
                    descriptor,
                    collector,
                    host,
                    last_refresh_at: None,
                }
            })
            .collect::<Vec<_>>();
        let hosts = collectors.iter().map(|state| state.host.clone()).collect();
        let pager = Pager::new(config.default_page_size);

        Self {
            config,
            hosts,
            pager,
            show_help: false,
            should_quit: false,
            collectors,
        }
    }

    pub fn refresh_all(&mut self) {
        self.refresh_due(true);
    }

    pub fn refresh_due(&mut self, force: bool) {
        let now = Instant::now();
        for state in &mut self.collectors {
            if !force && !state.is_due(now, &self.config) {
                continue;
            }

            let fallback = HostInfo::loading(state.descriptor.clone());
            state.host = match state.collector.collect() {
                    Ok(host) => host,
                    Err(error) => {
                        let mut failed = fallback;
                        failed.status = crate::model::HostStatus::Error;
                        failed.last_error = Some(error.to_string());
                        failed
                    }
                };
            state.last_refresh_at = Some(now);
        }

        self.rebuild_hosts();
    }

    fn rebuild_hosts(&mut self) {
        self.hosts = self.collectors.iter().map(|state| state.host.clone()).collect();
        sort_hosts(&mut self.hosts, self.config.ssh.unreachable_to_end);
        self.pager.clamp(self.hosts.len());
    }

    pub fn handle_key(&mut self, code: KeyCode) {
        if key_matches(&self.config.keys.quit, &code) {
            self.should_quit = true;
            return;
        }

        if key_matches(&self.config.keys.help, &code) {
            self.show_help = !self.show_help;
            return;
        }

        if key_matches(&self.config.keys.refresh, &code) {
            self.refresh_all();
            return;
        }

        if key_matches(&self.config.keys.next_page, &code) || code == KeyCode::Right {
            self.pager.next_page(self.hosts.len());
            return;
        }

        if key_matches(&self.config.keys.prev_page, &code) || code == KeyCode::Left {
            self.pager.prev_page(self.hosts.len());
        }
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }
}

impl CollectorState {
    fn is_due(&self, now: Instant, config: &AppConfig) -> bool {
        let interval = match self.descriptor.host_type {
            HostType::Local => Duration::from_millis(config.local_refresh_interval_ms),
            HostType::Remote => Duration::from_millis(config.remote_refresh_interval_ms),
        };

        self.last_refresh_at
            .map(|last_refresh_at| now.duration_since(last_refresh_at) >= interval)
            .unwrap_or(true)
    }
}

pub fn run(config: AppConfig) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let result = run_event_loop(&mut terminal, config);
    restore_terminal(terminal)?;
    result
}

fn run_event_loop(terminal: &mut Terminal<CrosstermBackend<Stdout>>, config: AppConfig) -> Result<()> {
    let mut app = App::new(config);
    app.refresh_all();

    let tick_rate = Duration::from_millis(250);
    let mut last_tick = Instant::now();

    loop {
        let page_size = compute_page_size(terminal.size()?.width, app.config.default_page_size);
        app.pager.set_page_size(page_size);
        app.pager.clamp(app.hosts.len());

        terminal.draw(|frame| ui::render(frame, &app))?;

        if app.should_quit() {
            break;
        }

        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    app.handle_key(key.code);
                }
            }
        }

        if last_tick.elapsed() >= tick_rate {
            app.refresh_due(false);
            last_tick = Instant::now();
        }
    }

    Ok(())
}

fn restore_terminal(mut terminal: Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn key_matches(binding: &str, code: &KeyCode) -> bool {
    match code {
        KeyCode::Char(value) => binding.eq_ignore_ascii_case(&value.to_string()),
        KeyCode::Esc => binding.eq_ignore_ascii_case("esc"),
        KeyCode::Enter => binding.eq_ignore_ascii_case("enter"),
        KeyCode::Left => binding.eq_ignore_ascii_case("left"),
        KeyCode::Right => binding.eq_ignore_ascii_case("right"),
        _ => false,
    }
}

fn compute_page_size(width: u16, configured_page_size: usize) -> usize {
    let max_by_terminal = if width >= 150 {
        3
    } else if width >= 90 {
        2
    } else {
        1
    };
    configured_page_size.min(max_by_terminal).max(1)
}

fn sort_hosts(hosts: &mut [HostInfo], unreachable_to_end: bool) {
    if !unreachable_to_end {
        return;
    }

    hosts.sort_by_key(|host| match (host.host_type, host.status) {
        (HostType::Local, _) => (0_u8, 0_u8),
        (HostType::Remote, HostStatus::Online | HostStatus::Loading) => (1_u8, 0_u8),
        (HostType::Remote, HostStatus::Unreachable) => (2_u8, 0_u8),
        (HostType::Remote, HostStatus::Error) => (3_u8, 0_u8),
    });
}
