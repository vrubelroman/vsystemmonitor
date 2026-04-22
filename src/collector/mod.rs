pub mod local;

use anyhow::Result;

use crate::model::{HostDescriptor, HostInfo};

pub trait HostCollector {
    fn descriptor(&self) -> HostDescriptor;
    fn collect(&mut self) -> Result<HostInfo>;
}
