use async_trait::async_trait;
use chrono::Utc;
use futures::StreamExt;
use nm_core::speedtest::{SpeedTestBackendKind, SpeedTestResult};
use reqwest::Client;
use std::time::Instant;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum SpeedTestError {
    #[error("speed test failed: {0}")]
    Failed(String),
}

#[async_trait]
pub trait SpeedTestBackend: Send + Sync {
    fn name(&self) -> &'static str;
    async fn run(
        &self,
        agent_name: &str,
        network_name: Option<&str>,
        interface: &str,
    ) -> Result<SpeedTestResult, SpeedTestError>;
}

pub fn create_backend(kind: SpeedTestBackendKind, server_url: Option<String>) -> Box<dyn SpeedTestBackend> {
    match kind {
        SpeedTestBackendKind::LibreSpeed => {
            Box::new(LibreSpeedBackend::new(server_url.unwrap_or_else(|| {
                "http://localhost".into()
            })))
        }
        SpeedTestBackendKind::Cloudflare => Box::new(CloudflareBackend::new()),
    }
}

pub struct CloudflareBackend {
    client: Client,
}

impl CloudflareBackend {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .unwrap_or_default(),
        }
    }
}

impl Default for CloudflareBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SpeedTestBackend for CloudflareBackend {
    fn name(&self) -> &'static str {
        "cloudflare"
    }

    async fn run(
        &self,
        agent_name: &str,
        network_name: Option<&str>,
        interface: &str,
    ) -> Result<SpeedTestResult, SpeedTestError> {
        let start = Instant::now();
        let latency_ms = measure_latency(&self.client).await;

        let download_url =
            "https://speed.cloudflare.com/__down?bytes=10000000";
        let download_start = Instant::now();
        let mut resp = self
            .client
            .get(download_url)
            .send()
            .await
            .map_err(|e| SpeedTestError::Failed(e.to_string()))?;
        let mut downloaded: u64 = 0;
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| SpeedTestError::Failed(e.to_string()))?
        {
            downloaded += chunk.len() as u64;
        }
        let download_secs = download_start.elapsed().as_secs_f64();
        let download_mbps = if download_secs > 0.0 {
            (downloaded as f64 * 8.0) / download_secs / 1_000_000.0
        } else {
            0.0
        };

        let upload_body = vec![0u8; 1_000_000];
        let upload_start = Instant::now();
        self.client
            .post("https://speed.cloudflare.com/__up")
            .body(upload_body)
            .send()
            .await
            .map_err(|e| SpeedTestError::Failed(e.to_string()))?;
        let upload_secs = upload_start.elapsed().as_secs_f64();
        let upload_mbps = if upload_secs > 0.0 {
            (1_000_000_f64 * 8.0) / upload_secs / 1_000_000.0
        } else {
            0.0
        };

        Ok(SpeedTestResult {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            agent_name: agent_name.to_string(),
            network_name: network_name.map(str::to_string),
            interface: interface.to_string(),
            download_mbps: Some(download_mbps),
            upload_mbps: Some(upload_mbps),
            latency_ms,
            jitter_ms: None,
            packet_loss_pct: None,
            server_name: Some("cloudflare".into()),
            test_duration_ms: Some(start.elapsed().as_millis() as u64),
            error: None,
        })
    }
}

async fn measure_latency(client: &Client) -> Option<f64> {
    let start = Instant::now();
    client
        .get("https://speed.cloudflare.com/cdn-cgi/trace")
        .send()
        .await
        .ok()?;
    Some(start.elapsed().as_secs_f64() * 1000.0)
}

pub struct LibreSpeedBackend {
    server_url: String,
    client: Client,
}

impl LibreSpeedBackend {
    pub fn new(server_url: String) -> Self {
        Self {
            server_url,
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .unwrap_or_default(),
        }
    }
}

#[async_trait]
impl SpeedTestBackend for LibreSpeedBackend {
    fn name(&self) -> &'static str {
        "librespeed"
    }

    async fn run(
        &self,
        agent_name: &str,
        network_name: Option<&str>,
        interface: &str,
    ) -> Result<SpeedTestResult, SpeedTestError> {
        let start = Instant::now();
        let base = self.server_url.trim_end_matches('/');
        let download_url = format!("{base}/garbage?ckSize=25");
        let download_start = Instant::now();
        let resp = self
            .client
            .get(&download_url)
            .send()
            .await
            .map_err(|e| SpeedTestError::Failed(e.to_string()))?;
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| SpeedTestError::Failed(e.to_string()))?;
        let download_secs = download_start.elapsed().as_secs_f64();
        let download_mbps = (bytes.len() as f64 * 8.0) / download_secs.max(0.001) / 1_000_000.0;

        let upload_body = vec![0u8; 1_000_000];
        let upload_start = Instant::now();
        self.client
            .post(format!("{base}/empty?ckSize=1"))
            .body(upload_body)
            .send()
            .await
            .map_err(|e| SpeedTestError::Failed(e.to_string()))?;
        let upload_secs = upload_start.elapsed().as_secs_f64();
        let upload_mbps = (1_000_000_f64 * 8.0) / upload_secs.max(0.001) / 1_000_000.0;

        Ok(SpeedTestResult {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            agent_name: agent_name.to_string(),
            network_name: network_name.map(str::to_string),
            interface: interface.to_string(),
            download_mbps: Some(download_mbps),
            upload_mbps: Some(upload_mbps),
            latency_ms: None,
            jitter_ms: None,
            packet_loss_pct: None,
            server_name: Some(base.to_string()),
            test_duration_ms: Some(start.elapsed().as_millis() as u64),
            error: None,
        })
    }
}
