use std::{
    collections::HashSet,
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::Duration,
    time::SystemTime,
};

use anyhow::{Context, Result};

use crate::{
    collector::{disks::parse_physical_disks_json, HostCollector},
    config::AppConfig,
    model::{HostDescriptor, HostInfo, HostStatus, HostType, MetricsSnapshot},
};

pub fn load_remote_collectors(config: &AppConfig) -> Result<Vec<RemoteCollector>> {
    let config_path = expand_home_path(&config.ssh.config_path)?;
    if !config_path.exists() {
        return Ok(Vec::new());
    }

    let hosts = parse_ssh_config(&config_path)
        .with_context(|| format!("failed to parse SSH config at {}", config_path.display()))?;

    Ok(hosts
        .into_iter()
        .map(|host| RemoteCollector::new(config, config_path.clone(), host))
        .collect())
}

pub struct RemoteCollector {
    descriptor: HostDescriptor,
    ssh_alias: String,
    ping_host: String,
    ssh_config_path: PathBuf,
    ssh_connect_timeout_ms: u64,
    host_ping_timeout_ms: u64,
    prefer_ssh_over_ping_check: bool,
}

impl RemoteCollector {
    fn new(config: &AppConfig, ssh_config_path: PathBuf, host: SshHostEntry) -> Self {
        Self {
            descriptor: HostDescriptor {
                id: format!("remote:{}", host.alias),
                alias: host.alias.clone(),
                display_name: host.alias.clone(),
                host_type: HostType::Remote,
            },
            ssh_alias: host.alias.clone(),
            ping_host: host.host_name.unwrap_or_else(|| host.alias.clone()),
            ssh_config_path,
            ssh_connect_timeout_ms: config.ssh.ssh_connect_timeout_ms,
            host_ping_timeout_ms: config.ssh.host_ping_timeout_ms,
            prefer_ssh_over_ping_check: config.ssh.prefer_ssh_over_ping_check,
        }
    }

    fn collect_remote_metrics(&self) -> std::result::Result<MetricsSnapshot, RemoteCollectError> {
        for attempt in 0..=1 {
            match self.collect_remote_metrics_once() {
                Ok(metrics) => return Ok(metrics),
                Err(error) if attempt == 0 && error.is_retryable() => {
                    thread::sleep(Duration::from_millis(300));
                }
                Err(error) => return Err(error),
            }
        }

        Err(RemoteCollectError::Error("unexpected remote retry state".to_string()))
    }

    fn collect_remote_metrics_once(&self) -> std::result::Result<MetricsSnapshot, RemoteCollectError> {
        if !self.prefer_ssh_over_ping_check {
            self.check_ping()?;
        }

        let mut child = Command::new("ssh")
            .arg("-F")
            .arg(&self.ssh_config_path)
            .arg("-o")
            .arg("BatchMode=yes")
            .arg("-o")
            .arg("NumberOfPasswordPrompts=0")
            .arg("-o")
            .arg(format!(
                "ConnectTimeout={}",
                (self.ssh_connect_timeout_ms / 1_000).max(1)
            ))
            .arg(&self.ssh_alias)
            .arg("sh")
            .arg("-s")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| RemoteCollectError::Error(format!("failed to run ssh: {error}")))?;

        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| RemoteCollectError::Error("failed to open ssh stdin".to_string()))?;
        stdin
            .write_all(remote_metrics_script().as_bytes())
            .map_err(|error| RemoteCollectError::Error(format!("failed to send remote script: {error}")))?;
        drop(stdin);

        let output = child
            .wait_with_output()
            .map_err(|error| RemoteCollectError::Error(format!("failed to read ssh output: {error}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(classify_ssh_failure(&stderr));
        }

        parse_remote_metrics(
            &String::from_utf8_lossy(&output.stdout),
        )
    }

    fn check_ping(&self) -> std::result::Result<(), RemoteCollectError> {
        let output = Command::new("ping")
            .arg("-c")
            .arg("1")
            .arg("-W")
            .arg((self.host_ping_timeout_ms / 1_000).max(1).to_string())
            .arg(&self.ping_host)
            .output()
            .map_err(|error| RemoteCollectError::Error(format!("failed to run ping: {error}")))?;

        if output.status.success() {
            Ok(())
        } else {
            Err(RemoteCollectError::Unreachable("No ping".to_string()))
        }
    }

    fn host_with_status(
        &self,
        status: HostStatus,
        metrics: MetricsSnapshot,
        last_error: Option<String>,
    ) -> HostInfo {
        HostInfo {
            id: self.descriptor.id.clone(),
            alias: self.descriptor.alias.clone(),
            display_name: self.descriptor.display_name.clone(),
            host_type: self.descriptor.host_type,
            status,
            metrics,
            last_updated: Some(SystemTime::now()),
            last_error,
        }
    }
}

impl HostCollector for RemoteCollector {
    fn descriptor(&self) -> HostDescriptor {
        self.descriptor.clone()
    }

    fn collect(&mut self) -> Result<HostInfo> {
        match self.collect_remote_metrics() {
            Ok(metrics) => Ok(self.host_with_status(HostStatus::Online, metrics, None)),
            Err(RemoteCollectError::Unreachable(message)) => Ok(self.host_with_status(
                HostStatus::Unreachable,
                MetricsSnapshot::default(),
                Some(message),
            )),
            Err(RemoteCollectError::Error(message)) => {
                Ok(self.host_with_status(HostStatus::Error, MetricsSnapshot::default(), Some(message)))
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SshHostEntry {
    alias: String,
    host_name: Option<String>,
}

#[derive(Debug)]
enum RemoteCollectError {
    Unreachable(String),
    Error(String),
}

impl RemoteCollectError {
    fn is_retryable(&self) -> bool {
        matches!(self, RemoteCollectError::Unreachable(_))
    }
}

fn parse_ssh_config(path: &Path) -> Result<Vec<SshHostEntry>> {
    let raw = fs::read_to_string(path)?;
    let mut hosts = Vec::new();
    let mut current_aliases: Vec<String> = Vec::new();
    let mut current_host_name: Option<String> = None;
    let mut seen = HashSet::new();

    let flush_block =
        |hosts: &mut Vec<SshHostEntry>,
         seen: &mut HashSet<String>,
         aliases: &mut Vec<String>,
         host_name: &mut Option<String>| {
            for alias in aliases.drain(..) {
                if seen.insert(alias.clone()) {
                    hosts.push(SshHostEntry {
                        alias,
                        host_name: host_name.clone(),
                    });
                }
            }
            *host_name = None;
        };

    for line in raw.lines() {
        let line = strip_comments(line).trim();
        if line.is_empty() {
            continue;
        }

        let mut parts = line.split_whitespace();
        let Some(key) = parts.next() else {
            continue;
        };
        let value = parts.collect::<Vec<_>>().join(" ");
        if value.is_empty() {
            continue;
        }

        if key.eq_ignore_ascii_case("host") {
            flush_block(
                &mut hosts,
                &mut seen,
                &mut current_aliases,
                &mut current_host_name,
            );
            current_aliases = value
                .split_whitespace()
                .filter(|alias| is_explicit_alias(alias))
                .map(ToString::to_string)
                .collect();
        } else if key.eq_ignore_ascii_case("hostname") {
            current_host_name = Some(value.trim().to_string());
        }
    }

    flush_block(
        &mut hosts,
        &mut seen,
        &mut current_aliases,
        &mut current_host_name,
    );

    Ok(hosts)
}

fn strip_comments(line: &str) -> &str {
    match line.find('#') {
        Some(index) => &line[..index],
        None => line,
    }
}

fn is_explicit_alias(alias: &str) -> bool {
    !alias.is_empty() && !alias.contains('*') && !alias.contains('?') && !alias.starts_with('!')
}

fn expand_home_path(path: &str) -> Result<PathBuf> {
    if path == "~" {
        let home = env::var("HOME").context("HOME is not set")?;
        return Ok(PathBuf::from(home));
    }

    if let Some(rest) = path.strip_prefix("~/") {
        let home = env::var("HOME").context("HOME is not set")?;
        return Ok(Path::new(&home).join(rest));
    }

    Ok(PathBuf::from(path))
}

fn remote_metrics_script() -> &'static str {
    r#"prev_cpu="$(awk '/^cpu / {print $2+$3+$4+$5+$6+$7+$8, $5}' /proc/stat)"
prev_total="$(printf '%s\n' "$prev_cpu" | awk '{print $1}')"
prev_idle="$(printf '%s\n' "$prev_cpu" | awk '{print $2}')"
sleep 0.2
curr_cpu="$(awk '/^cpu / {print $2+$3+$4+$5+$6+$7+$8, $5}' /proc/stat)"
curr_total="$(printf '%s\n' "$curr_cpu" | awk '{print $1}')"
curr_idle="$(printf '%s\n' "$curr_cpu" | awk '{print $2}')"
delta_total=$((curr_total - prev_total))
delta_idle=$((curr_idle - prev_idle))
cpu_usage="$(awk -v total="$delta_total" -v idle="$delta_idle" 'BEGIN { if (total <= 0) printf "0.0"; else printf "%.1f", (total - idle) * 100 / total }')"
mem_total="$(awk '/^MemTotal:/ {printf "%.0f", $2 * 1024; exit}' /proc/meminfo)"
mem_available="$(awk '/^MemAvailable:/ {printf "%.0f", $2 * 1024; exit}' /proc/meminfo)"
mem_used=$((mem_total - mem_available))
cpu_temp=""
for file in /sys/class/hwmon/hwmon*/temp*_input; do
  [ -f "$file" ] || continue
  base="${file%_input}"
  dir="${base%/*}"
  label=""
  [ -f "$dir/name" ] && label="$(cat "$dir/name" 2>/dev/null)"
  [ -f "${base}_label" ] && label="$label $(cat "${base}_label" 2>/dev/null)"
  lowered="$(printf '%s' "$label" | tr '[:upper:]' '[:lower:]')"
  case "$lowered" in
    *package*|*cpu*|*core*|*tctl*|*tdie*|*ccd*|*k10temp*)
      raw="$(cat "$file" 2>/dev/null || true)"
      if [ -n "$raw" ]; then
        cpu_temp="$(awk -v value="$raw" 'BEGIN { printf "%.1f", value / 1000 }')"
        break
      fi
      ;;
  esac
done
if [ -z "$cpu_temp" ]; then
  for file in /sys/class/hwmon/hwmon*/temp*_input; do
    [ -f "$file" ] || continue
    raw="$(cat "$file" 2>/dev/null || true)"
    if [ -n "$raw" ]; then
      cpu_temp="$(awk -v value="$raw" 'BEGIN { printf "%.1f", value / 1000 }')"
      break
    fi
  done
fi
printf 'cpu_usage=%s\n' "$cpu_usage"
printf 'cpu_temp=%s\n' "$cpu_temp"
printf 'mem_used=%s\n' "$mem_used"
printf 'mem_total=%s\n' "$mem_total"
printf '__LSBLK_BEGIN__\n'
lsblk -J -b -o NAME,KNAME,PKNAME,PATH,TYPE,MOUNTPOINTS,SIZE,FSUSED 2>/dev/null || true
printf '\n__LSBLK_END__\n'
"#
}

fn classify_ssh_failure(stderr: &str) -> RemoteCollectError {
    let stderr = stderr.trim();
    let lowered = stderr.to_ascii_lowercase();

    if lowered.contains("permission denied") || lowered.contains("host key verification failed") {
        return RemoteCollectError::Error(compact_remote_error(stderr, "ssh authentication failed"));
    }

    if lowered.contains("timed out")
        || lowered.contains("no route to host")
        || lowered.contains("name or service not known")
        || lowered.contains("connection refused")
        || lowered.contains("could not resolve hostname")
        || lowered.contains("connection closed")
        || lowered.contains("connection reset")
        || lowered.contains("kex_exchange_identification")
        || lowered.contains("banner exchange")
        || lowered.contains("operation timed out")
    {
        return RemoteCollectError::Unreachable(compact_remote_error(stderr, "ssh timeout"));
    }

    RemoteCollectError::Error(compact_remote_error(stderr, "ssh failed"))
}

fn compact_remote_error(stderr: &str, fallback: &str) -> String {
    stderr
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .map(|line| line.trim().to_string())
        .unwrap_or_else(|| fallback.to_string())
}

fn parse_remote_metrics(stdout: &str) -> std::result::Result<MetricsSnapshot, RemoteCollectError> {
    let mut cpu_usage_percent = None;
    let mut cpu_temperature_celsius = None;
    let mut memory_used_bytes = None;
    let mut memory_total_bytes = None;
    let mut lsblk_lines = Vec::new();
    let mut in_lsblk = false;

    for raw_line in stdout.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }

        if line == "__LSBLK_BEGIN__" {
            in_lsblk = true;
            continue;
        }

        if line == "__LSBLK_END__" {
            in_lsblk = false;
            continue;
        }

        if in_lsblk {
            lsblk_lines.push(raw_line);
            continue;
        }

        if let Some(value) = line.strip_prefix("cpu_usage=") {
            cpu_usage_percent = Some(parse_f64(value, "cpu_usage")?);
            continue;
        }

        if let Some(value) = line.strip_prefix("cpu_temp=") {
            if !value.is_empty() {
                cpu_temperature_celsius = Some(parse_f64(value, "cpu_temp")?);
            }
            continue;
        }

        if let Some(value) = line.strip_prefix("mem_used=") {
            memory_used_bytes = Some(parse_u64(value, "mem_used")?);
            continue;
        }

        if let Some(value) = line.strip_prefix("mem_total=") {
            memory_total_bytes = Some(parse_u64(value, "mem_total")?);
            continue;
        }
    }

    let memory_total_bytes =
        memory_total_bytes.ok_or_else(|| RemoteCollectError::Error("missing mem_total".to_string()))?;
    let memory_used_bytes =
        memory_used_bytes.ok_or_else(|| RemoteCollectError::Error("missing mem_used".to_string()))?;

    let disks = if lsblk_lines.is_empty() {
        Vec::new()
    } else {
        parse_physical_disks_json(&lsblk_lines.join("\n"))
            .map_err(|error| RemoteCollectError::Error(error.to_string()))?
    };

    Ok(MetricsSnapshot {
        cpu_usage_percent: cpu_usage_percent
            .ok_or_else(|| RemoteCollectError::Error("missing cpu_usage".to_string()))?,
        cpu_temperature_celsius,
        memory_used_bytes,
        memory_total_bytes,
        memory_usage_percent: usage_percent(memory_used_bytes, memory_total_bytes),
        disks,
    })
}

fn parse_f64(value: &str, field: &str) -> std::result::Result<f64, RemoteCollectError> {
    value.trim().parse::<f64>().map_err(|error| {
        RemoteCollectError::Error(format!("failed to parse {field}: {error}"))
    })
}

fn parse_u64(value: &str, field: &str) -> std::result::Result<u64, RemoteCollectError> {
    value.trim().parse::<u64>().map_err(|error| {
        RemoteCollectError::Error(format!("failed to parse {field}: {error}"))
    })
}

fn usage_percent(used: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        used as f64 * 100.0 / total as f64
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_remote_metrics, parse_ssh_config};
    use std::{fs, path::PathBuf, time::{SystemTime, UNIX_EPOCH}};

    #[test]
    fn parses_basic_ssh_config_aliases() {
        let path = unique_test_path("ssh_config");
        fs::write(
            &path,
            "Host server\n    HostName 10.0.0.1\nHost *\n    ForwardAgent no\nHost app db\n    HostName internal\n",
        )
        .unwrap();

        let hosts = parse_ssh_config(&path).unwrap();
        fs::remove_file(path).unwrap();

        assert_eq!(hosts.len(), 3);
        assert_eq!(hosts[0].alias, "server");
        assert_eq!(hosts[0].host_name.as_deref(), Some("10.0.0.1"));
        assert_eq!(hosts[1].alias, "app");
        assert_eq!(hosts[2].alias, "db");
    }

    #[test]
    fn parses_remote_metrics_payload() {
        let payload = r#"\
cpu_usage=12.5
cpu_temp=58.0
mem_used=100
mem_total=200
__LSBLK_BEGIN__
{"blockdevices":[{"name":"sda","type":"disk","size":100,"fsused":null,"mountpoints":null,"children":[{"name":"sda1","type":"part","size":80,"fsused":50,"mountpoints":["/"]},{"name":"sda2","type":"part","size":20,"fsused":10,"mountpoints":["/boot"]}]}]}
__LSBLK_END__
"#;

        let metrics = parse_remote_metrics(payload).unwrap();
        assert_eq!(metrics.cpu_usage_percent, 12.5);
        assert_eq!(metrics.cpu_temperature_celsius, Some(58.0));
        assert_eq!(metrics.memory_usage_percent, 50.0);
        assert_eq!(metrics.disks.len(), 1);
        assert_eq!(metrics.disks[0].mount_point, "/,/boot");
    }

    fn unique_test_path(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}_{nanos}.tmp"))
    }
}
