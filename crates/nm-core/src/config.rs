use crate::speedtest::SpeedTestConfig;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub agent: AgentConfig,
    pub network: NetworkConfig,
    pub speedtest: SpeedTestConfig,
    pub storage: StorageConfig,
    pub api: ApiConfig,
    pub notifications: NotificationsConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            agent: AgentConfig::default(),
            network: NetworkConfig::default(),
            speedtest: SpeedTestConfig::default(),
            storage: StorageConfig::default(),
            api: ApiConfig::default(),
            notifications: NotificationsConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub name: Option<String>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self { name: None }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub interface: String,
    pub scan_interval_secs: u64,
    pub passive_capture: bool,
    pub gateway: String,
    pub presence_timeout_secs: u64,
    pub link_check_interval_secs: u64,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            interface: "auto".into(),
            scan_interval_secs: 60,
            passive_capture: true,
            gateway: "auto".into(),
            presence_timeout_secs: 180,
            link_check_interval_secs: 30,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    pub database_path: String,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            database_path: "~/.local/share/network-monitor/network-monitor.db".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiConfig {
    pub bind_addr: String,
    pub enabled: bool,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            bind_addr: "127.0.0.1:8080".into(),
            enabled: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NotificationsConfig {
    pub ntfy: NtfyConfig,
    pub webhook: WebhookConfig,
    pub desktop: DesktopConfig,
    pub fcm: FcmConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NtfyConfig {
    pub enabled: bool,
    pub server: String,
    pub topic: String,
}

impl Default for NtfyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            server: "https://ntfy.sh".into(),
            topic: "home-lan".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookConfig {
    pub enabled: bool,
    pub url: String,
    pub secret: Option<String>,
}

impl Default for WebhookConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            url: String::new(),
            secret: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesktopConfig {
    pub enabled: bool,
}

impl Default for DesktopConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FcmConfig {
    pub enabled: bool,
    pub project_id: Option<String>,
    pub credentials_path: Option<String>,
}

impl Default for FcmConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            project_id: None,
            credentials_path: None,
        }
    }
}

pub fn expand_path(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return format!("{home}/{rest}");
        }
    }
    path.to_string()
}

pub fn default_agent_name_from_hostname() -> String {
    hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .map(|h| {
            h.split(['-', '_'])
                .map(|part| {
                    let mut c = part.chars();
                    match c.next() {
                        None => String::new(),
                        Some(f) => f.to_uppercase().chain(c).collect(),
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_else(|| "Network Monitor".into())
}

pub fn load_config(path: &Path) -> Result<AppConfig, crate::CoreError> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| crate::CoreError::Config(format!("read {}: {e}", path.display())))?;
    toml::from_str(&content).map_err(|e| crate::CoreError::Config(e.to_string()))
}

pub fn save_config(path: &Path, config: &AppConfig) -> Result<(), crate::CoreError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| crate::CoreError::Config(format!("create dir: {e}")))?;
    }
    let content =
        toml::to_string_pretty(config).map_err(|e| crate::CoreError::Config(e.to_string()))?;
    std::fs::write(path, content)
        .map_err(|e| crate::CoreError::Config(format!("write {}: {e}", path.display())))
}

pub fn resolve_secret(value: &Option<String>) -> Option<String> {
    value.as_ref().and_then(|v| {
        if let Some(key) = v.strip_prefix("env:") {
            std::env::var(key).ok()
        } else {
            Some(v.clone())
        }
    })
}
