use std::{
    env,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Clone, Debug)]
pub struct AppConfig {
    pub local_refresh_interval_ms: u64,
    pub remote_refresh_interval_ms: u64,
    pub theme: ThemeName,
    pub show_borders: bool,
    pub compact_mode: bool,
    pub show_help_hints: bool,
    pub default_page_size: usize,
    pub cpu_warning_threshold: f64,
    pub cpu_critical_threshold: f64,
    pub cpu_temp_warning_threshold: f64,
    pub cpu_temp_critical_threshold: f64,
    pub ram_warning_threshold: f64,
    pub ram_critical_threshold: f64,
    pub disk_warning_threshold: f64,
    pub disk_critical_threshold: f64,
    pub stale_data_timeout_ms: u64,
    pub show_all_disks: bool,
    pub disk_include_mountpoints: Vec<String>,
    pub disk_exclude_mountpoints: Vec<String>,
    pub keys: KeyBindings,
    pub ssh: SshConfig,
}

#[derive(Clone, Debug)]
pub struct KeyBindings {
    pub next_page: String,
    pub prev_page: String,
    pub refresh: String,
    pub quit: String,
    pub help: String,
}

#[derive(Clone, Debug)]
pub struct SshConfig {
    pub config_path: String,
    pub ssh_connect_timeout_ms: u64,
    pub host_ping_timeout_ms: u64,
    pub unreachable_to_end: bool,
    pub prefer_ssh_over_ping_check: bool,
    pub max_parallel_hosts: usize,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeName {
    CatppuccinMocha,
}

impl ThemeName {
    pub fn as_str(&self) -> &'static str {
        match self {
            ThemeName::CatppuccinMocha => "catppuccin_mocha",
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            local_refresh_interval_ms: 2_000,
            remote_refresh_interval_ms: 15_000,
            theme: ThemeName::CatppuccinMocha,
            show_borders: true,
            compact_mode: false,
            show_help_hints: true,
            default_page_size: 3,
            cpu_warning_threshold: 60.0,
            cpu_critical_threshold: 85.0,
            cpu_temp_warning_threshold: 55.0,
            cpu_temp_critical_threshold: 70.0,
            ram_warning_threshold: 60.0,
            ram_critical_threshold: 85.0,
            disk_warning_threshold: 70.0,
            disk_critical_threshold: 90.0,
            stale_data_timeout_ms: 20_000,
            show_all_disks: true,
            disk_include_mountpoints: vec!["/".to_string()],
            disk_exclude_mountpoints: vec!["/boot".to_string()],
            keys: KeyBindings::default(),
            ssh: SshConfig::default(),
        }
    }
}

impl Default for KeyBindings {
    fn default() -> Self {
        Self {
            next_page: "l".to_string(),
            prev_page: "h".to_string(),
            refresh: "r".to_string(),
            quit: "q".to_string(),
            help: "?".to_string(),
        }
    }
}

impl Default for SshConfig {
    fn default() -> Self {
        Self {
            config_path: "~/.ssh/config".to_string(),
            ssh_connect_timeout_ms: 5_000,
            host_ping_timeout_ms: 1_000,
            unreachable_to_end: true,
            prefer_ssh_over_ping_check: true,
            max_parallel_hosts: 8,
        }
    }
}

impl AppConfig {
    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }

        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read config from {}", path.display()))?;
        let parsed: RawAppConfig = toml::from_str(&raw)
            .with_context(|| format!("failed to parse TOML config at {}", path.display()))?;

        Ok(parsed.merge(Self::default()))
    }

    pub fn config_path() -> Result<PathBuf> {
        if let Ok(path) = env::var("XDG_CONFIG_HOME") {
            return Ok(Path::new(&path).join("vsysmonitor").join("config.toml"));
        }

        let home = env::var("HOME").context("HOME is not set and XDG_CONFIG_HOME is unavailable")?;
        Ok(Path::new(&home)
            .join(".config")
            .join("vsysmonitor")
            .join("config.toml"))
    }
}

#[derive(Debug, Default, Deserialize)]
struct RawAppConfig {
    refresh_interval_ms: Option<u64>,
    local_refresh_interval_ms: Option<u64>,
    remote_refresh_interval_ms: Option<u64>,
    theme: Option<ThemeName>,
    show_borders: Option<bool>,
    compact_mode: Option<bool>,
    show_help_hints: Option<bool>,
    default_page_size: Option<usize>,
    cpu_warning_threshold: Option<f64>,
    cpu_critical_threshold: Option<f64>,
    cpu_temp_warning_threshold: Option<f64>,
    cpu_temp_critical_threshold: Option<f64>,
    ram_warning_threshold: Option<f64>,
    ram_critical_threshold: Option<f64>,
    disk_warning_threshold: Option<f64>,
    disk_critical_threshold: Option<f64>,
    stale_data_timeout_ms: Option<u64>,
    show_all_disks: Option<bool>,
    disk_include_mountpoints: Option<Vec<String>>,
    disk_exclude_mountpoints: Option<Vec<String>>,
    keys: Option<RawKeyBindings>,
    ssh: Option<RawSshConfig>,
}

#[derive(Debug, Default, Deserialize)]
struct RawKeyBindings {
    next_page: Option<String>,
    prev_page: Option<String>,
    refresh: Option<String>,
    quit: Option<String>,
    help: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawSshConfig {
    config_path: Option<String>,
    ssh_connect_timeout_ms: Option<u64>,
    host_ping_timeout_ms: Option<u64>,
    unreachable_to_end: Option<bool>,
    prefer_ssh_over_ping_check: Option<bool>,
    max_parallel_hosts: Option<usize>,
}

impl RawAppConfig {
    fn merge(self, mut defaults: AppConfig) -> AppConfig {
        let legacy_refresh_interval_ms = self.refresh_interval_ms;
        defaults.local_refresh_interval_ms = self
            .local_refresh_interval_ms
            .or(legacy_refresh_interval_ms)
            .unwrap_or(defaults.local_refresh_interval_ms);
        defaults.remote_refresh_interval_ms = self
            .remote_refresh_interval_ms
            .or(legacy_refresh_interval_ms)
            .unwrap_or(defaults.remote_refresh_interval_ms);
        defaults.theme = self.theme.unwrap_or(defaults.theme);
        defaults.show_borders = self.show_borders.unwrap_or(defaults.show_borders);
        defaults.compact_mode = self.compact_mode.unwrap_or(defaults.compact_mode);
        defaults.show_help_hints = self.show_help_hints.unwrap_or(defaults.show_help_hints);
        defaults.default_page_size = self.default_page_size.unwrap_or(defaults.default_page_size).max(1);
        defaults.cpu_warning_threshold = self.cpu_warning_threshold.unwrap_or(defaults.cpu_warning_threshold);
        defaults.cpu_critical_threshold = self.cpu_critical_threshold.unwrap_or(defaults.cpu_critical_threshold);
        defaults.cpu_temp_warning_threshold =
            self.cpu_temp_warning_threshold.unwrap_or(defaults.cpu_temp_warning_threshold);
        defaults.cpu_temp_critical_threshold =
            self.cpu_temp_critical_threshold.unwrap_or(defaults.cpu_temp_critical_threshold);
        defaults.ram_warning_threshold = self.ram_warning_threshold.unwrap_or(defaults.ram_warning_threshold);
        defaults.ram_critical_threshold = self.ram_critical_threshold.unwrap_or(defaults.ram_critical_threshold);
        defaults.disk_warning_threshold = self.disk_warning_threshold.unwrap_or(defaults.disk_warning_threshold);
        defaults.disk_critical_threshold = self.disk_critical_threshold.unwrap_or(defaults.disk_critical_threshold);
        defaults.stale_data_timeout_ms = self.stale_data_timeout_ms.unwrap_or(defaults.stale_data_timeout_ms);
        defaults.show_all_disks = self.show_all_disks.unwrap_or(defaults.show_all_disks);
        defaults.disk_include_mountpoints =
            self.disk_include_mountpoints.unwrap_or(defaults.disk_include_mountpoints);
        defaults.disk_exclude_mountpoints =
            self.disk_exclude_mountpoints.unwrap_or(defaults.disk_exclude_mountpoints);

        if let Some(keys) = self.keys {
            defaults.keys.next_page = keys.next_page.unwrap_or(defaults.keys.next_page);
            defaults.keys.prev_page = keys.prev_page.unwrap_or(defaults.keys.prev_page);
            defaults.keys.refresh = keys.refresh.unwrap_or(defaults.keys.refresh);
            defaults.keys.quit = keys.quit.unwrap_or(defaults.keys.quit);
            defaults.keys.help = keys.help.unwrap_or(defaults.keys.help);
        }

        if let Some(ssh) = self.ssh {
            defaults.ssh.config_path = ssh.config_path.unwrap_or(defaults.ssh.config_path);
            defaults.ssh.ssh_connect_timeout_ms =
                ssh.ssh_connect_timeout_ms.unwrap_or(defaults.ssh.ssh_connect_timeout_ms);
            defaults.ssh.host_ping_timeout_ms =
                ssh.host_ping_timeout_ms.unwrap_or(defaults.ssh.host_ping_timeout_ms);
            defaults.ssh.unreachable_to_end =
                ssh.unreachable_to_end.unwrap_or(defaults.ssh.unreachable_to_end);
            defaults.ssh.prefer_ssh_over_ping_check =
                ssh.prefer_ssh_over_ping_check.unwrap_or(defaults.ssh.prefer_ssh_over_ping_check);
            defaults.ssh.max_parallel_hosts =
                ssh.max_parallel_hosts.unwrap_or(defaults.ssh.max_parallel_hosts).max(1);
        }

        defaults
    }
}
