pub mod discord;

use crate::config::ConfigManager;
use anyhow::Result;

pub async fn run() -> Result<()> {
    discord::run().await
}

pub async fn run_with_config(config: ConfigManager) -> Result<()> {
    discord::run_with_config(config).await
}
