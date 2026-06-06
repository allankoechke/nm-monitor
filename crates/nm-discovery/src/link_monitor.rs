use crate::backend::check_gateway_reachable;
use crate::interface::detect_network;
use parking_lot::RwLock;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;
use tracing::info;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    Up,
    Down,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct LinkStatus {
    pub state: LinkState,
    pub gateway: Option<IpAddr>,
    pub interface_up: bool,
}

pub struct LinkMonitor {
    interface: String,
    gateway: String,
    check_interval_secs: u64,
    status: Arc<RwLock<LinkStatus>>,
}

impl LinkMonitor {
    pub fn new(interface: &str, gateway: &str, check_interval_secs: u64) -> Self {
        Self {
            interface: interface.to_string(),
            gateway: gateway.to_string(),
            check_interval_secs,
            status: Arc::new(RwLock::new(LinkStatus {
                state: LinkState::Unknown,
                gateway: None,
                interface_up: true,
            })),
        }
    }

    pub fn status(&self) -> LinkStatus {
        self.status.read().clone()
    }

    pub fn spawn(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(self.check_interval_secs.max(5)));
            let mut last_state = LinkState::Unknown;
            loop {
                ticker.tick().await;
                let network = detect_network(&self.interface, &self.gateway).ok();
                let gateway = network.as_ref().and_then(|n| n.gateway);
                let interface_up = network.is_some();
                let reachable = if let Some(gw) = gateway {
                    check_gateway_reachable(gw).await
                } else {
                    false
                };
                let state = if interface_up && reachable {
                    LinkState::Up
                } else {
                    LinkState::Down
                };
                {
                    let mut s = self.status.write();
                    s.state = state;
                    s.gateway = gateway;
                    s.interface_up = interface_up;
                }
                if state != last_state && last_state != LinkState::Unknown {
                    info!(?state, ?gateway, "link state changed");
                }
                last_state = state;
            }
        })
    }
}
