use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpeedTestBackendKind {
    LibreSpeed,
    Cloudflare,
}

impl std::str::FromStr for SpeedTestBackendKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "librespeed" => Ok(Self::LibreSpeed),
            "cloudflare" => Ok(Self::Cloudflare),
            other => Err(format!("unknown speedtest backend: {other}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeedTestConfig {
    pub enabled: bool,
    pub interval_secs: u64,
    pub backend: SpeedTestBackendKind,
    pub server_url: Option<String>,
    pub min_interval_secs: u64,
}

impl Default for SpeedTestConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_secs: 21_600,
            backend: SpeedTestBackendKind::Cloudflare,
            server_url: None,
            min_interval_secs: 300,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeedTestResult {
    pub id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub agent_name: String,
    pub network_name: Option<String>,
    pub interface: String,
    pub download_mbps: Option<f64>,
    pub upload_mbps: Option<f64>,
    pub latency_ms: Option<f64>,
    pub jitter_ms: Option<f64>,
    pub packet_loss_pct: Option<f64>,
    pub server_name: Option<String>,
    pub test_duration_ms: Option<u64>,
    pub error: Option<String>,
}
