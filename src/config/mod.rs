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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_config_new() {
        let config = ServerConfig::new("test_server");
        assert_eq!(config.server_id, "test_server");
        assert!(config.auto_embed_channels.is_empty());
        assert!(config.embed_enabled);
    }

    #[test]
    fn test_server_config_default() {
        let config = ServerConfig::default();
        assert_eq!(config.server_id, "");
        assert!(config.auto_embed_channels.is_empty());
        assert!(config.embed_enabled);
    }

    #[test]
    fn test_server_config_is_auto_embed_channel_found() {
        let mut config = ServerConfig::new("test_server");
        config.auto_embed_channels =
            HashSet::from(["channel1".to_string(), "channel2".to_string()]);

        assert!(config.is_auto_embed_channel("channel1"));
        assert!(config.is_auto_embed_channel("channel2"));
    }

    #[test]
    fn test_server_config_is_auto_embed_channel_not_found() {
        let mut config = ServerConfig::new("test_server");
        config.auto_embed_channels = HashSet::from(["channel1".to_string()]);

        assert!(!config.is_auto_embed_channel("channel2"));
        assert!(!config.is_auto_embed_channel("nonexistent"));
    }

    #[test]
    fn test_config_get_discord_token_some() {
        let config = Config {
            discord: Some(DiscordConfig {
                token: Some("test_token".to_string()),
            }),
            servers: vec![],
            logging: None,
        };

        assert_eq!(config.get_discord_token(), Some("test_token".to_string()));
    }

    #[test]
    fn test_config_get_discord_token_none() {
        let config = Config {
            discord: None,
            servers: vec![],
            logging: None,
        };

        assert!(config.get_discord_token().is_none());
    }

    #[test]
    fn test_config_get_discord_token_none_inner() {
        let config = Config {
            discord: Some(DiscordConfig { token: None }),
            servers: vec![],
            logging: None,
        };

        assert!(config.get_discord_token().is_none());
    }

    #[test]
    fn test_config_get_logging_format_custom() {
        let config = Config {
            discord: None,
            servers: vec![],
            logging: Some(LoggingConfig {
                format: Some("pretty".to_string()),
                level: None,
            }),
        };

        assert_eq!(config.get_logging_format(), "pretty");
    }

    #[test]
    fn test_config_get_logging_format_none() {
        let config = Config {
            discord: None,
            servers: vec![],
            logging: None,
        };

        assert_eq!(config.get_logging_format(), "json");
    }

    #[test]
    fn test_config_get_logging_format_none_inner() {
        let config = Config {
            discord: None,
            servers: vec![],
            logging: Some(LoggingConfig {
                format: None,
                level: None,
            }),
        };

        assert_eq!(config.get_logging_format(), "json");
    }

    #[test]
    fn test_config_get_log_level_custom() {
        let config = Config {
            discord: None,
            servers: vec![],
            logging: Some(LoggingConfig {
                format: None,
                level: Some("debug".to_string()),
            }),
        };

        assert_eq!(config.get_log_level(), "debug");
    }

    #[test]
    fn test_config_get_log_level_none() {
        let config = Config {
            discord: None,
            servers: vec![],
            logging: None,
        };

        assert_eq!(config.get_log_level(), "info");
    }

    #[test]
    fn test_config_get_log_level_none_inner() {
        let config = Config {
            discord: None,
            servers: vec![],
            logging: Some(LoggingConfig {
                format: None,
                level: None,
            }),
        };

        assert_eq!(config.get_log_level(), "info");
    }

    #[test]
    fn test_config_from_file_valid_toml() {
        let toml_content = r#"
            [discord]
            token = "test_token"

            [logging]
            format = "pretty"
            level = "debug"

            [[servers]]
            server_id = "server1"
            auto_embed_channels = ["channel1", "channel2"]
            embed_enabled = true

            [[servers]]
            server_id = "server2"
            auto_embed_channels = []
            embed_enabled = false
        "#;

        let temp_file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(temp_file.path(), toml_content).unwrap();

        let config = Config::from_file(temp_file.path()).unwrap();

        assert_eq!(config.get_discord_token(), Some("test_token".to_string()));
        assert_eq!(config.get_logging_format(), "pretty");
        assert_eq!(config.get_log_level(), "debug");
        assert_eq!(config.servers.len(), 2);
        assert_eq!(config.servers[0].server_id, "server1");
        assert_eq!(config.servers[1].server_id, "server2");
    }

    #[test]
    fn test_config_from_file_minimal() {
        let toml_content = r#"
            [[servers]]
            server_id = "server1"
            auto_embed_channels = []
            embed_enabled = true
        "#;

        let temp_file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(temp_file.path(), toml_content).unwrap();

        let config = Config::from_file(temp_file.path()).unwrap();

        assert!(config.get_discord_token().is_none());
        assert_eq!(config.get_logging_format(), "json");
        assert_eq!(config.get_log_level(), "info");
        assert_eq!(config.servers.len(), 1);
    }

    #[test]
    fn test_config_from_file_invalid_toml() {
        let toml_content = r#"
            [discord
            token = "invalid
        "#;

        let temp_file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(temp_file.path(), toml_content).unwrap();

        let result = Config::from_file(temp_file.path());
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Failed to parse config file"));
    }

    #[test]
    fn test_config_from_file_not_found() {
        let result = Config::from_file("/nonexistent/path/config.toml");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Failed to read config file"));
    }

    #[test]
    fn test_config_manager_new() {
        let manager = ConfigManager::new();
        assert!(manager.configs.is_empty());
    }

    #[test]
    fn test_config_manager_from_config_file() {
        let toml_content = r#"
            [[servers]]
            server_id = "server1"
            auto_embed_channels = ["channel1"]
            embed_enabled = true

            [[servers]]
            server_id = "server2"
            auto_embed_channels = []
            embed_enabled = false
        "#;

        let temp_file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(temp_file.path(), toml_content).unwrap();

        let manager = ConfigManager::from_config_file(temp_file.path()).unwrap();

        assert_eq!(manager.configs.len(), 2);
        assert!(manager.configs.contains_key("server1"));
        assert!(manager.configs.contains_key("server2"));
    }

    #[test]
    fn test_config_manager_get_server_config_existing() {
        let toml_content = r#"
            [[servers]]
            server_id = "server1"
            auto_embed_channels = ["channel1"]
            embed_enabled = false
        "#;

        let temp_file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(temp_file.path(), toml_content).unwrap();

        let manager = ConfigManager::from_config_file(temp_file.path()).unwrap();
        let config = manager.get_server_config("server1");

        assert_eq!(config.server_id, "server1");
        assert!(!config.embed_enabled);
        assert!(config.is_auto_embed_channel("channel1"));
    }

    #[test]
    fn test_config_manager_get_server_config_default() {
        let manager = ConfigManager::new();
        let config = manager.get_server_config("new_server");

        assert_eq!(config.server_id, "new_server");
        assert!(config.auto_embed_channels.is_empty());
        assert!(config.embed_enabled);
    }

    #[test]
    fn test_config_manager_is_auto_embed_channel_true() {
        let toml_content = r#"
            [[servers]]
            server_id = "guild1"
            auto_embed_channels = ["channel1", "channel2"]
            embed_enabled = true
        "#;

        let temp_file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(temp_file.path(), toml_content).unwrap();

        let manager = ConfigManager::from_config_file(temp_file.path()).unwrap();

        assert!(manager.is_auto_embed_channel("guild1", "channel1"));
        assert!(manager.is_auto_embed_channel("guild1", "channel2"));
    }

    #[test]
    fn test_config_manager_is_auto_embed_channel_false() {
        let toml_content = r#"
            [[servers]]
            server_id = "guild1"
            auto_embed_channels = ["channel1"]
            embed_enabled = true
        "#;

        let temp_file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(temp_file.path(), toml_content).unwrap();

        let manager = ConfigManager::from_config_file(temp_file.path()).unwrap();

        assert!(!manager.is_auto_embed_channel("guild1", "channel2"));
        assert!(!manager.is_auto_embed_channel("guild2", "channel1"));
    }

    #[test]
    fn test_config_manager_is_auto_embed_channel_default_guild() {
        let manager = ConfigManager::new();
        assert!(!manager.is_auto_embed_channel("any_guild", "any_channel"));
    }
}
