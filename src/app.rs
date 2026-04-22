use std::{
    io::{self, Stdout},
    sync::mpsc::{self, Receiver, SyncSender, TrySendError},
    thread,
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
    ui,
};

pub struct App {
    pub config: AppConfig,
    pub hosts: Vec<HostInfo>,
    pub selected_host_id: Option<String>,
    pub show_help: bool,
    should_quit: bool,
    result_rx: Receiver<WorkerResult>,
    collectors: Vec<CollectorState>,
}

struct CollectorState {
    descriptor: HostDescriptor,
    host: HostInfo,
    last_refresh_at: Option<Instant>,
    refresh_tx: SyncSender<()>,
    pending: bool,
}

struct WorkerResult {
    host_id: String,
    host: HostInfo,
}

impl App {
    pub fn new(config: AppConfig) -> Self {
        let (result_tx, result_rx) = mpsc::channel();
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
            .map(|mut collector| {
                let descriptor = collector.descriptor();
                let (refresh_tx, refresh_rx) = mpsc::sync_channel::<()>(1);
                let host = if descriptor.host_type == HostType::Local {
                    collect_with_fallback(&mut *collector, &descriptor)
                } else {
                    HostInfo::loading(descriptor.clone())
                };
                let last_refresh_at = (descriptor.host_type == HostType::Local).then(Instant::now);
                spawn_collector_worker(collector, descriptor.clone(), refresh_rx, result_tx.clone());
                CollectorState {
                    descriptor,
                    host,
                    last_refresh_at,
                    refresh_tx,
                    pending: false,
                }
            })
            .collect::<Vec<_>>();
        let hosts = sorted_hosts(&collectors, config.ssh.unreachable_to_end);
        let selected_host_id = hosts.first().map(|host| host.id.clone());
        let mut app = Self {
            config,
            hosts,
            selected_host_id,
            show_help: false,
            should_quit: false,
            result_rx,
            collectors,
        };
        app.request_initial_remote_refreshes();
        app
    }

    pub fn refresh_all(&mut self) {
        self.poll_updates();
        self.request_due_refreshes(true);
    }

    pub fn refresh_due(&mut self, force: bool) {
        self.poll_updates();
        self.request_due_refreshes(force);
    }

    fn rebuild_hosts(&mut self) {
        self.hosts = sorted_hosts(&self.collectors, self.config.ssh.unreachable_to_end);
        self.clamp_selection();
    }

    fn request_initial_remote_refreshes(&mut self) {
        for state in &mut self.collectors {
            if state.descriptor.host_type == HostType::Remote {
                state.request_refresh();
            }
        }
    }

    fn request_due_refreshes(&mut self, force: bool) {
        let now = Instant::now();
        for state in &mut self.collectors {
            if force || state.is_due(now, &self.config) {
                state.request_refresh();
            }
        }
    }

    fn poll_updates(&mut self) {
        let mut changed = false;
        while let Ok(result) = self.result_rx.try_recv() {
            if let Some(state) = self
                .collectors
                .iter_mut()
                .find(|state| state.descriptor.id == result.host_id)
            {
                state.host = result.host;
                state.last_refresh_at = Some(Instant::now());
                state.pending = false;
                changed = true;
            }
        }

        if changed {
            self.rebuild_hosts();
        }
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

        if key_matches(&self.config.keys.next_page, &code)
            || matches!(code, KeyCode::Right | KeyCode::Down)
        {
            self.select_next_host();
            return;
        }

        if key_matches(&self.config.keys.prev_page, &code)
            || matches!(code, KeyCode::Left | KeyCode::Up)
        {
            self.select_prev_host();
        }
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn selected_host(&self) -> Option<&HostInfo> {
        let selected_host_id = self.selected_host_id.as_deref()?;
        self.hosts.iter().find(|host| host.id == selected_host_id)
    }

    pub fn selected_host_index(&self) -> usize {
        let Some(selected_host_id) = self.selected_host_id.as_deref() else {
            return 0;
        };

        self.hosts
            .iter()
            .position(|host| host.id == selected_host_id)
            .unwrap_or(0)
    }

    fn select_next_host(&mut self) {
        if self.hosts.is_empty() {
            self.selected_host_id = None;
            return;
        }

        let next_index = (self.selected_host_index() + 1) % self.hosts.len();
        self.selected_host_id = Some(self.hosts[next_index].id.clone());
    }

    fn select_prev_host(&mut self) {
        if self.hosts.is_empty() {
            self.selected_host_id = None;
            return;
        }

        let current_index = self.selected_host_index();
        let prev_index = if current_index == 0 {
            self.hosts.len() - 1
        } else {
            current_index - 1
        };
        self.selected_host_id = Some(self.hosts[prev_index].id.clone());
    }

    fn clamp_selection(&mut self) {
        if self.hosts.is_empty() {
            self.selected_host_id = None;
            return;
        }

        let selected_host_id = self.selected_host_id.clone();
        if let Some(selected_host_id) = selected_host_id {
            if self.hosts.iter().any(|host| host.id == selected_host_id) {
                return;
            }
        }

        self.selected_host_id = Some(self.hosts[0].id.clone());
    }
}

impl CollectorState {
    fn is_due(&self, now: Instant, config: &AppConfig) -> bool {
        if self.pending {
            return false;
        }

        let interval = match self.descriptor.host_type {
            HostType::Local => Duration::from_millis(config.local_refresh_interval_ms),
            HostType::Remote => Duration::from_millis(config.remote_refresh_interval_ms),
        };

        self.last_refresh_at
            .map(|last_refresh_at| now.duration_since(last_refresh_at) >= interval)
            .unwrap_or(true)
    }

    fn request_refresh(&mut self) {
        if self.pending {
            return;
        }

        match self.refresh_tx.try_send(()) {
            Ok(()) => {
                self.pending = true;
            }
            Err(TrySendError::Full(_)) => {
                self.pending = true;
            }
            Err(TrySendError::Disconnected(_)) => {
                self.pending = false;
            }
        }
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

    let tick_rate = Duration::from_millis(250);
    let mut last_tick = Instant::now();

    loop {
        app.poll_updates();
        terminal.size()?;

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

fn sorted_hosts(collectors: &[CollectorState], unreachable_to_end: bool) -> Vec<HostInfo> {
    let mut hosts = collectors.iter().map(|state| state.host.clone()).collect::<Vec<_>>();
    if !unreachable_to_end {
        return hosts;
    }

    hosts.sort_by_key(|host| match (host.host_type, host.status) {
        (HostType::Local, _) => (0_u8, 0_u8),
        (HostType::Remote, HostStatus::Online | HostStatus::Loading) => (1_u8, 0_u8),
        (HostType::Remote, HostStatus::Unreachable) => (2_u8, 0_u8),
        (HostType::Remote, HostStatus::Error) => (3_u8, 0_u8),
    });
    hosts
}

fn spawn_collector_worker(
    mut collector: Box<dyn HostCollector>,
    descriptor: HostDescriptor,
    refresh_rx: Receiver<()>,
    result_tx: mpsc::Sender<WorkerResult>,
) {
    thread::spawn(move || {
        while refresh_rx.recv().is_ok() {
            let host = collect_with_fallback(&mut *collector, &descriptor);
            if result_tx
                .send(WorkerResult {
                    host_id: descriptor.id.clone(),
                    host,
                })
                .is_err()
            {
                break;
            }
        }
    });
}

fn collect_with_fallback(collector: &mut dyn HostCollector, descriptor: &HostDescriptor) -> HostInfo {
    match collector.collect() {
        Ok(host) => host,
        Err(error) => {
            let mut failed = HostInfo::loading(descriptor.clone());
            failed.status = HostStatus::Error;
            failed.last_error = Some(error.to_string());
            failed
        }
    }
}
