use std::time::SystemTime;

use anyhow::Result;
use sysinfo::{CpuExt, DiskExt, System, SystemExt};

use crate::{
    collector::HostCollector,
    config::AppConfig,
    model::{DiskInfo, HostDescriptor, HostInfo, HostStatus, HostType, MetricsSnapshot},
};

pub struct LocalCollector {
    descriptor: HostDescriptor,
    system: System,
    show_all_disks: bool,
    include_mountpoints: Vec<String>,
    exclude_mountpoints: Vec<String>,
}

impl LocalCollector {
    pub fn new(config: &AppConfig) -> Self {
        let mut system = System::new_all();
        system.refresh_all();

        let hostname = system.host_name().unwrap_or_else(|| "localhost".to_string());
        Self {
            descriptor: HostDescriptor {
                id: "local".to_string(),
                alias: "local".to_string(),
                display_name: hostname,
                host_type: HostType::Local,
            },
            system,
            show_all_disks: config.show_all_disks,
            include_mountpoints: config.disk_include_mountpoints.clone(),
            exclude_mountpoints: config.disk_exclude_mountpoints.clone(),
        }
    }

    fn should_keep_disk(&self, mount_point: &str) -> bool {
        if self.exclude_mountpoints.iter().any(|item| item == mount_point) {
            return false;
        }

        if self.show_all_disks {
            return true;
        }

        self.include_mountpoints.iter().any(|item| item == mount_point)
    }
}

impl HostCollector for LocalCollector {
    fn descriptor(&self) -> HostDescriptor {
        self.descriptor.clone()
    }

    fn collect(&mut self) -> Result<HostInfo> {
        self.system.refresh_cpu();
        self.system.refresh_memory();
        self.system.refresh_disks_list();
        self.system.refresh_disks();

        let total_memory = self.system.total_memory();
        let used_memory = self.system.used_memory();
        let memory_usage_percent = if total_memory == 0 {
            0.0
        } else {
            used_memory as f64 * 100.0 / total_memory as f64
        };

        let disks = self
            .system
            .disks()
            .iter()
            .filter_map(|disk| {
                let mount_point = disk.mount_point().to_string_lossy().to_string();
                if !self.should_keep_disk(&mount_point) {
                    return None;
                }

                let total_bytes = disk.total_space();
                let available_bytes = disk.available_space();
                let used_bytes = total_bytes.saturating_sub(available_bytes);
                let usage_percent = if total_bytes == 0 {
                    0.0
                } else {
                    used_bytes as f64 * 100.0 / total_bytes as f64
                };

                Some(DiskInfo {
                    name: disk.name().to_string_lossy().to_string(),
                    mount_point,
                    used_bytes,
                    total_bytes,
                    usage_percent,
                })
            })
            .collect();

        Ok(HostInfo {
            id: self.descriptor.id.clone(),
            alias: self.descriptor.alias.clone(),
            display_name: self.descriptor.display_name.clone(),
            host_type: self.descriptor.host_type,
            status: HostStatus::Online,
            metrics: MetricsSnapshot {
                cpu_usage_percent: self.system.global_cpu_info().cpu_usage() as f64,
                memory_used_bytes: used_memory,
                memory_total_bytes: total_memory,
                memory_usage_percent,
                disks,
            },
            last_updated: Some(SystemTime::now()),
            last_error: None,
        })
    }
}
