use anyhow::{Context, Result};
use clap::Parser;
use tracing::info;
use tracing_subscriber::filter::{EnvFilter, LevelFilter};

mod bot;
mod config;
mod media;
mod utils;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to the config file
    #[arg(short, long)]
    config: Option<String>,
}

fn get_config_path(args: &Args) -> Option<String> {
    if let Some(path) = &args.config {
        return Some(path.clone());
    }

    if let Ok(path) = std::env::var("CONFIG_FILE") {
        return Some(path);
    }

    if let Ok(xdg_config_home) = std::env::var("XDG_CONFIG_HOME") {
        let config_dir = format!("{}/grabby", xdg_config_home);
        let config_path = format!("{}/config.yaml", config_dir);
        if std::path::Path::new(&config_path).exists() {
            return Some(config_path);
        }
    }

    if let Some(home) = dirs::home_dir() {
        let config_dir = format!("{}/.config/grabby", home.display());
        let config_path = format!("{}/config.yaml", config_dir);
        if std::path::Path::new(&config_path).exists() {
            return Some(config_path);
        }
    }

    None
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let env_filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env_lossy();

    let log_format = if let Some(config_path) = get_config_path(&args) {
        let config_file = crate::config::Config::from_file(&config_path)?;
        config_file.get_logging_format().to_string()
    } else {
        "json".to_string()
    };

    if log_format == "json" {
        tracing_subscriber::fmt()
            .json()
            .with_env_filter(env_filter)
            .init();
    } else {
        tracing_subscriber::fmt().with_env_filter(env_filter).init();
    }

    info!("Starting Grabby...");

    if let Some(config_path) = get_config_path(&args) {
        info!("Loading config from: {}", config_path);
        let config_file = crate::config::Config::from_file(&config_path)
            .with_context(|| format!("Failed to load config from {}", config_path))?;
        let config = crate::config::ConfigManager::from_config_file(&config_path)
            .with_context(|| format!("Failed to load config from {}", config_path))?;

        if let Some(token) = config_file.get_discord_token() {
            std::env::set_var("DISCORD_TOKEN", token);
        }

        bot::run_with_config(config).await?;
    } else {
        info!("No config file found, running without server configuration");
        bot::run().await?;
    }

    Ok(())
}
