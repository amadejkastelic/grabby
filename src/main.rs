use anyhow::Result;
use tracing::info;
use tracing_subscriber::filter::{EnvFilter, LevelFilter};

mod bot;
mod config;
mod media;

#[tokio::main]
async fn main() -> Result<()> {
    let env_filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env_lossy();
    tracing_subscriber::fmt().with_env_filter(env_filter).init();

    info!("Starting Grabby...");

    // Start the bot (Discord for now, but extensible)
    bot::run().await?;

    Ok(())
}
