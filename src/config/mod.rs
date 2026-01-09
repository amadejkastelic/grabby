use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ServerConfig {
    pub server_id: String,
    pub auto_embed_channels: HashSet<String>,
    pub embed_enabled: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            server_id: String::new(),
            auto_embed_channels: HashSet::new(),
            embed_enabled: true,
        }
    }
}

impl ServerConfig {
    pub fn new(server_id: &str) -> Self {
        Self {
            server_id: server_id.to_string(),
            auto_embed_channels: HashSet::new(),
            embed_enabled: true,
        }
    }

    pub fn is_auto_embed_channel(&self, channel_id: &str) -> bool {
        self.auto_embed_channels.iter().any(|id| id == channel_id)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct LoggingConfig {
    pub format: Option<String>,
    pub level: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct DiscordConfig {
    pub token: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Config {
    pub discord: Option<DiscordConfig>,
    pub servers: Vec<ServerConfig>,
    pub logging: Option<LoggingConfig>,
}

impl Config {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;

        let config: Config = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))?;

        Ok(config)
    }

    pub fn get_discord_token(&self) -> Option<String> {
        self.discord.as_ref().and_then(|d| d.token.clone())
    }

    pub fn get_logging_format(&self) -> &str {
        self.logging
            .as_ref()
            .and_then(|l| l.format.as_deref())
            .unwrap_or("json")
    }

    pub fn get_log_level(&self) -> &str {
        self.logging
            .as_ref()
            .and_then(|l| l.level.as_deref())
            .unwrap_or("info")
    }
}

pub struct ConfigManager {
    configs: HashMap<String, ServerConfig>,
}

impl ConfigManager {
    pub fn new() -> Self {
        Self {
            configs: HashMap::new(),
        }
    }

    pub fn from_config_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let config = Config::from_file(path)?;
        let configs: HashMap<String, ServerConfig> = config
            .servers
            .into_iter()
            .map(|s| (s.server_id.clone(), s))
            .collect();

        Ok(Self { configs })
    }

    pub fn get_server_config(&self, server_id: &str) -> ServerConfig {
        self.configs
            .get(server_id)
            .cloned()
            .unwrap_or_else(|| ServerConfig::new(server_id))
    }

    pub fn is_auto_embed_channel(&self, guild_id: &str, channel_id: &str) -> bool {
        self.get_server_config(guild_id)
            .is_auto_embed_channel(channel_id)
    }
}
