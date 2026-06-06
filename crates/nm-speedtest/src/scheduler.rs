use crate::backend::{create_backend, SpeedTestBackend, SpeedTestError};
use chrono::Utc;
use nm_core::speedtest::{SpeedTestConfig, SpeedTestResult};
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::interval;
use tracing::{error, info};
use uuid::Uuid;

#[derive(Clone)]
pub struct SpeedTestContext {
    pub agent_name: String,
    pub network_name: Option<String>,
    pub interface: String,
    pub network_up: bool,
}

pub struct SpeedTestScheduler {
    config: SpeedTestConfig,
    backend: Box<dyn SpeedTestBackend>,
    context: Arc<Mutex<SpeedTestContext>>,
    last_manual: Arc<Mutex<Option<Instant>>>,
}

impl SpeedTestScheduler {
    pub fn new(config: SpeedTestConfig, context: SpeedTestContext) -> Self {
        let backend = create_backend(config.backend, config.server_url.clone());
        Self {
            config,
            backend,
            context: Arc::new(Mutex::new(context)),
            last_manual: Arc::new(Mutex::new(None)),
        }
    }

    pub fn update_context(&self, ctx: SpeedTestContext) {
        *self.context.lock() = ctx;
    }

    pub async fn run_once(&self) -> SpeedTestResult {
        let ctx = self.context.lock().clone();
        if !ctx.network_up {
            return SpeedTestResult {
                id: Uuid::new_v4(),
                timestamp: Utc::now(),
                agent_name: ctx.agent_name,
                network_name: ctx.network_name,
                interface: ctx.interface,
                download_mbps: None,
                upload_mbps: None,
                latency_ms: None,
                jitter_ms: None,
                packet_loss_pct: None,
                server_name: None,
                test_duration_ms: None,
                error: Some("network is down".into()),
            };
        }
        match self
            .backend
            .run(&ctx.agent_name, ctx.network_name.as_deref(), &ctx.interface)
            .await
        {
            Ok(result) => result,
            Err(SpeedTestError::Failed(msg)) => SpeedTestResult {
                id: Uuid::new_v4(),
                timestamp: Utc::now(),
                agent_name: ctx.agent_name,
                network_name: ctx.network_name,
                interface: ctx.interface,
                download_mbps: None,
                upload_mbps: None,
                latency_ms: None,
                jitter_ms: None,
                packet_loss_pct: None,
                server_name: None,
                test_duration_ms: None,
                error: Some(msg),
            },
        }
    }

    pub fn can_run_manual(&self) -> bool {
        let last = self.last_manual.lock();
        match *last {
            Some(t) => t.elapsed().as_secs() >= self.config.min_interval_secs,
            None => true,
        }
    }

    pub async fn run_manual(&self) -> Result<SpeedTestResult, String> {
        if !self.can_run_manual() {
            return Err(format!(
                "rate limited — wait at least {} seconds between manual tests",
                self.config.min_interval_secs
            ));
        }
        *self.last_manual.lock() = Some(Instant::now());
        Ok(self.run_once().await)
    }

    pub fn spawn<F>(self: Arc<Self>, on_result: F) -> tokio::task::JoinHandle<()>
    where
        F: Fn(SpeedTestResult) + Send + Sync + 'static,
    {
        tokio::spawn(async move {
            if !self.config.enabled {
                info!("speed tests disabled");
                return;
            }
            let mut ticker = interval(Duration::from_secs(self.config.interval_secs.max(300)));
            ticker.tick().await;
            loop {
                ticker.tick().await;
                let result = self.run_once().await;
                if let Some(err) = &result.error {
                    error!(error = %err, "speed test failed");
                } else {
                    info!(
                        download = ?result.download_mbps,
                        upload = ?result.upload_mbps,
                        "speed test completed"
                    );
                }
                on_result(result);
            }
        })
    }
}
