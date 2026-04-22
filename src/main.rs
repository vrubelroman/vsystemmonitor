mod app;
mod collector;
mod config;
mod model;
mod navigation;
mod theme;
mod ui;

use anyhow::Result;
use config::AppConfig;

fn main() -> Result<()> {
    let config = AppConfig::load()?;
    app::run(config)
}
