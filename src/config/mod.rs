use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

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
        self.auto_embed_channels.contains(channel_id)
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
