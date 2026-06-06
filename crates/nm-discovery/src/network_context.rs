use parking_lot::RwLock;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;
use tracing::{debug, warn};

#[derive(Debug, Clone, Default)]
pub struct NetworkContextState {
    pub network_name: Option<String>,
    pub interface: Option<String>,
}

#[derive(Clone)]
pub struct NetworkContext {
    state: Arc<RwLock<NetworkContextState>>,
}

impl NetworkContext {
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(NetworkContextState::default())),
        }
    }

    pub fn network_name(&self) -> Option<String> {
        self.state.read().network_name.clone()
    }

    pub fn interface(&self) -> Option<String> {
        self.state.read().interface.clone()
    }

    pub fn set_interface(&self, interface: &str) {
        self.state.write().interface = Some(interface.to_string());
        self.refresh_ssid(interface);
    }

    pub fn refresh_ssid(&self, interface: &str) {
        let ssid = detect_wifi_ssid(interface);
        debug!(interface, ?ssid, "refreshed network context");
        self.state.write().network_name = ssid;
    }

    pub fn spawn_refresh_task(self, interface: String, poll_secs: u64) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(poll_secs.max(30)));
            loop {
                ticker.tick().await;
                self.refresh_ssid(&interface);
            }
        })
    }
}

fn detect_wifi_ssid(interface: &str) -> Option<String> {
    if let Some(ssid) = ssid_via_nmcli(interface) {
        return Some(ssid);
    }
    ssid_via_iw(interface)
}

fn ssid_via_nmcli(interface: &str) -> Option<String> {
    let output = std::process::Command::new("nmcli")
        .args(["-t", "-f", "ACTIVE,SSID", "dev", "wifi"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        let mut parts = line.split(':');
        let active = parts.next()?;
        let ssid = parts.next()?;
        if active == "yes" && !ssid.is_empty() {
            return Some(ssid.to_string());
        }
    }

    let output = std::process::Command::new("nmcli")
        .args(["-t", "-f", "GENERAL.CONNECTION", "device", "show", interface])
        .output()
        .ok()?;
    if output.status.success() {
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            if let Some(conn) = line.strip_prefix("GENERAL.CONNECTION:") {
                let conn = conn.trim();
                if !conn.is_empty() && conn != "--" {
                    return Some(conn.to_string());
                }
            }
        }
    }
    None
}

fn ssid_via_iw(interface: &str) -> Option<String> {
    let output = std::process::Command::new("iw")
        .args(["dev", interface, "link"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        if let Some(ssid) = line.trim().strip_prefix("SSID:") {
            let ssid = ssid.trim();
            if !ssid.is_empty() {
                return Some(ssid.to_string());
            }
        }
    }
    warn!("could not detect WiFi SSID for interface {interface}");
    None
}
