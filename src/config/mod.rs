use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ServerConfig {
    pub server_id: String,
    pub auto_embed_channels: HashSet<String>,
    pub embed_enabled: bool,
    #[serde(default)]
    pub disabled_domains: HashSet<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            server_id: String::new(),
            auto_embed_channels: HashSet::new(),
            embed_enabled: true,
            disabled_domains: HashSet::new(),
        }
    }
}

impl ServerConfig {
    pub fn new(server_id: &str) -> Self {
        Self {
            server_id: server_id.to_string(),
            auto_embed_channels: HashSet::new(),
            embed_enabled: true,
            disabled_domains: HashSet::new(),
        }
    }

    pub fn is_auto_embed_channel(&self, channel_id: &str) -> bool {
        self.auto_embed_channels.iter().any(|id| id == channel_id)
    }

    pub fn is_domain_disabled(&self, url: &str) -> bool {
        if self.disabled_domains.is_empty() {
            return false;
        }

        if let Some(host) = Self::extract_host(url) {
            let host_lower = host.to_lowercase();

            for disabled in &self.disabled_domains {
                let disabled_lower = disabled.to_lowercase();

                // Exact match
                if host_lower == disabled_lower {
                    return true;
                }

                // Subdomain match (e.g., if "example.com" is disabled, "sub.example.com" is also disabled)
                if host_lower.ends_with(&format!(".{}", disabled_lower)) {
                    return true;
                }
            }
        }

        false
    }

    fn extract_host(url: &str) -> Option<String> {
        let without_protocol = if let Some(pos) = url.find("://") {
            &url[pos + 3..]
        } else {
            url
        };

        let host = without_protocol.split('/').next()?.split(':').next()?;

        if host.is_empty() {
            None
        } else {
            Some(host.to_string())
        }
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
    fn test_is_domain_disabled_empty() {
        let config = ServerConfig::new("test_server");
        assert!(!config.is_domain_disabled("https://example.com"));
    }

    #[test]
    fn test_is_domain_disabled_exact_match() {
        let mut config = ServerConfig::new("test_server");
        config.disabled_domains.insert("example.com".to_string());

        assert!(config.is_domain_disabled("https://example.com"));
        assert!(config.is_domain_disabled("http://example.com"));
        assert!(config.is_domain_disabled("https://example.com/path"));
    }

    #[test]
    fn test_is_domain_disabled_subdomain() {
        let mut config = ServerConfig::new("test_server");
        config.disabled_domains.insert("example.com".to_string());

        assert!(config.is_domain_disabled("https://sub.example.com"));
        assert!(config.is_domain_disabled("https://deep.sub.example.com"));
        assert!(!config.is_domain_disabled("https://example.com.au"));
    }

    #[test]
    fn test_is_domain_disabled_case_insensitive() {
        let mut config = ServerConfig::new("test_server");
        config.disabled_domains.insert("EXAMPLE.COM".to_string());

        assert!(config.is_domain_disabled("https://example.com"));
        assert!(config.is_domain_disabled("https://Example.Com"));
        assert!(config.is_domain_disabled("https://SUB.example.com"));
    }

    #[test]
    fn test_is_domain_disabled_not_disabled() {
        let mut config = ServerConfig::new("test_server");
        config.disabled_domains.insert("example.com".to_string());

        assert!(!config.is_domain_disabled("https://other.com"));
        assert!(!config.is_domain_disabled("https://notexample.com"));
    }

    #[test]
    fn test_is_domain_disabled_with_port() {
        let mut config = ServerConfig::new("test_server");
        config.disabled_domains.insert("example.com".to_string());

        assert!(config.is_domain_disabled("https://example.com:8080"));
        assert!(config.is_domain_disabled("http://example.com:3000/path"));
    }

    #[test]
    fn test_config_from_file_with_disabled_domains() {
        let toml_content = r#"
            [[servers]]
            server_id = "server1"
            auto_embed_channels = ["channel1"]
            embed_enabled = true
            disabled_domains = ["example.com", "test.org"]
        "#;

        let temp_file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(temp_file.path(), toml_content).unwrap();

        let config = Config::from_file(temp_file.path()).unwrap();
        assert_eq!(config.servers.len(), 1);
        assert!(config.servers[0].disabled_domains.contains("example.com"));
        assert!(config.servers[0].disabled_domains.contains("test.org"));
    }
}
