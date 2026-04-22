use std::time::SystemTime;

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct HostInfo {
    pub id: String,
    pub alias: String,
    pub display_name: String,
    pub host_type: HostType,
    pub status: HostStatus,
    pub metrics: MetricsSnapshot,
    pub last_updated: Option<SystemTime>,
    pub last_error: Option<String>,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostType {
    Local,
    Remote,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostStatus {
    Online,
    Loading,
    Unreachable,
    Error,
}

#[derive(Clone, Debug, Default)]
pub struct MetricsSnapshot {
    pub cpu_usage_percent: f64,
    pub memory_used_bytes: u64,
    pub memory_total_bytes: u64,
    pub memory_usage_percent: f64,
    pub disks: Vec<DiskInfo>,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct DiskInfo {
    pub name: String,
    pub mount_point: String,
    pub used_bytes: u64,
    pub total_bytes: u64,
    pub usage_percent: f64,
}

#[derive(Clone, Debug)]
pub struct HostDescriptor {
    pub id: String,
    pub alias: String,
    pub display_name: String,
    pub host_type: HostType,
}

impl HostInfo {
    pub fn loading(descriptor: HostDescriptor) -> Self {
        Self {
            id: descriptor.id,
            alias: descriptor.alias,
            display_name: descriptor.display_name,
            host_type: descriptor.host_type,
            status: HostStatus::Loading,
            metrics: MetricsSnapshot::default(),
            last_updated: None,
            last_error: None,
        }
    }
}
