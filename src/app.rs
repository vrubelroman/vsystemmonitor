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
    collector::{local::LocalCollector, HostCollector},
    config::AppConfig,
    model::HostInfo,
    navigation::Pager,
    ui,
};

pub struct App {
    pub config: AppConfig,
    pub hosts: Vec<HostInfo>,
    pub pager: Pager,
    pub show_help: bool,
    should_quit: bool,
    collectors: Vec<Box<dyn HostCollector>>,
}

impl App {
    pub fn new(config: AppConfig) -> Self {
        let collectors: Vec<Box<dyn HostCollector>> = vec![Box::new(LocalCollector::new(&config))];
        let hosts = collectors
            .iter()
            .map(|collector| HostInfo::loading(collector.descriptor()))
            .collect();
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

    pub fn refresh(&mut self) {
        self.hosts = self
            .collectors
            .iter_mut()
            .map(|collector| {
                let fallback = HostInfo::loading(collector.descriptor());
                match collector.collect() {
                    Ok(host) => host,
                    Err(error) => {
                        let mut failed = fallback;
                        failed.status = crate::model::HostStatus::Error;
                        failed.last_error = Some(error.to_string());
                        failed
                    }
                }
            })
            .collect();
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
            self.refresh();
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
    app.refresh();

    let tick_rate = Duration::from_millis(app.config.refresh_interval_ms);
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
            app.refresh();
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
