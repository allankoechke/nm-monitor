use nm_discovery::{LinkMonitor, NetworkContext};
use nm_speedtest::SpeedTestScheduler;
use nm_store::Store;
use parking_lot::RwLock;
use std::sync::Arc;
use tokio::sync::broadcast;

#[derive(Clone)]
pub struct ApiState {
    pub store: Arc<Store>,
    pub network_context: NetworkContext,
    pub link_monitor: Arc<LinkMonitor>,
    pub agent_name: Arc<RwLock<String>>,
    pub speedtest: Option<Arc<SpeedTestScheduler>>,
    pub event_tx: broadcast::Sender<String>,
}
