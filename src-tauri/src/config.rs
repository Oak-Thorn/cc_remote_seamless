use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{info, warn};

#[derive(Debug, Deserialize, Default, Clone)]
pub struct AppConfig {
    pub general: Option<GeneralConfig>,
    pub platforms: HashMap<String, PlatformConfig>,
    pub agents: HashMap<String, AgentConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct GeneralConfig {
    pub hook_port: Option<u16>,
    #[serde(default)]
    pub sounds: SoundConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SoundConfig {
    #[serde(default = "default_idle_sound")]
    pub idle: String,
    #[serde(default = "default_permission_sound")]
    pub permission: String,
}

fn default_idle_sound() -> String { "Glass".to_string() }
fn default_permission_sound() -> String { "Hero".to_string() }

impl Default for SoundConfig {
    fn default() -> Self {
        Self {
            idle: default_idle_sound(),
            permission: default_permission_sound(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum PlatformConfig {
    #[serde(rename = "feishu")]
    Feishu {
        app_id: String,
        app_secret: String,
        chat_id: String,
    },
    #[serde(rename = "telegram")]
    Telegram {
        bot_token: String,
        chat_id: String,
    },
}

impl PlatformConfig {
    pub fn chat_id(&self) -> &str {
        match self {
            PlatformConfig::Feishu { chat_id, .. } => chat_id,
            PlatformConfig::Telegram { chat_id, .. } => chat_id,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct AgentConfig {
    pub platforms: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigInfo {
    pub hook_port: u16,
    pub platform_count: usize,
}

impl AppConfig {
    pub fn hook_port(&self) -> u16 {
        self.general.as_ref().and_then(|g| g.hook_port).unwrap_or(23399)
    }

    pub fn to_info(&self) -> ConfigInfo {
        ConfigInfo {
            hook_port: self.hook_port(),
            platform_count: self.platforms.len(),
        }
    }

    pub fn platforms_for_agent(&self, agent_id: &str) -> Vec<&str> {
        self.agents.get(agent_id)
            .map(|a| a.platforms.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default()
    }
}

fn config_path() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|d| d.home_dir().join(".cc-remote").join("config.toml"))
}

pub fn config_path_string() -> String {
    config_path()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default()
}

pub fn load() -> AppConfig {
    let path = match config_path() {
        Some(p) => p,
        None => {
            warn!("Cannot determine home directory, using default config");
            return AppConfig::default();
        }
    };

    if !path.exists() {
        info!("Config file not found at {:?}, using defaults", path);
        return AppConfig::default();
    }

    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            warn!("Failed to read config {:?}: {}", path, e);
            return AppConfig::default();
        }
    };

    match toml::from_str::<AppConfig>(&content) {
        Ok(config) => {
            info!("Loaded config from {:?}: {} platforms, {} agents",
                path, config.platforms.len(), config.agents.len());
            config
        }
        Err(e) => {
            warn!("Failed to parse config {:?}: {}", path, e);
            AppConfig::default()
        }
    }
}
