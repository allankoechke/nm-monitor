use crate::arp::arp_sweep;
use crate::interface::{detect_network, NetworkInfo};
use crate::ping::ping_host;
use async_trait::async_trait;
use mac_address::MacAddress;
use nm_core::device::DeviceSnapshot;
use nm_classify::oui::lookup_vendor;
use std::net::IpAddr;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DiscoveryError {
    #[error("discovery failed: {0}")]
    Failed(String),
    #[error("no suitable network interface found")]
    NoInterface,
}

#[derive(Debug, Clone)]
pub struct DiscoverySnapshot {
    pub network: NetworkInfo,
    pub devices: Vec<DeviceSnapshot>,
}

#[async_trait]
pub trait DiscoveryBackend: Send + Sync {
    async fn discover(&self) -> Result<DiscoverySnapshot, DiscoveryError>;
    fn platform_name(&self) -> &'static str;
}

pub struct LinuxDiscoveryBackend {
    pub interface: String,
    pub gateway: String,
}

impl LinuxDiscoveryBackend {
    pub fn new(interface: &str, gateway: &str) -> Self {
        Self {
            interface: interface.to_string(),
            gateway: gateway.to_string(),
        }
    }
}

#[async_trait]
impl DiscoveryBackend for LinuxDiscoveryBackend {
    async fn discover(&self) -> Result<DiscoverySnapshot, DiscoveryError> {
        let network = detect_network(&self.interface, &self.gateway)
            .map_err(|e| DiscoveryError::Failed(e.to_string()))?;
        let arp_results = arp_sweep(&network.interface, &network.subnet)
            .map_err(|e| DiscoveryError::Failed(e.to_string()))?;

        let mut devices = Vec::new();
        for (ip, mac) in arp_results {
            let vendor = lookup_vendor(&mac);
            devices.push(DeviceSnapshot {
                mac,
                ip: Some(ip),
                hostname: reverse_dns(ip).await,
                vendor,
            });
        }

        Ok(DiscoverySnapshot { network, devices })
    }

    fn platform_name(&self) -> &'static str {
        "linux"
    }
}

/// Cross-platform backend selector. Linux is implemented; other platforms return stubs.
pub struct PlatformBackend {
    inner: LinuxDiscoveryBackend,
}

impl PlatformBackend {
    pub fn new(interface: &str, gateway: &str) -> Self {
        Self {
            inner: LinuxDiscoveryBackend::new(interface, gateway),
        }
    }
}

#[async_trait]
impl DiscoveryBackend for PlatformBackend {
    async fn discover(&self) -> Result<DiscoverySnapshot, DiscoveryError> {
        #[cfg(target_os = "linux")]
        {
            return self.inner.discover().await;
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = &self.inner;
            Err(DiscoveryError::Failed(
                "discovery not yet implemented for this platform".into(),
            ))
        }
    }

    fn platform_name(&self) -> &'static str {
        #[cfg(target_os = "linux")]
        {
            return "linux";
        }
        #[cfg(target_os = "macos")]
        {
            return "macos-stub";
        }
        #[cfg(target_os = "windows")]
        {
            return "windows-stub";
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        {
            "unknown"
        }
    }
}

pub async fn check_gateway_reachable(gateway: IpAddr) -> bool {
    ping_host(gateway, 1_000).await.unwrap_or(false)
}

async fn reverse_dns(ip: IpAddr) -> Option<String> {
    tokio::task::spawn_blocking(move || {
        use std::net::ToSocketAddrs;
        let addr = format!("{ip}:0");
        if let Ok(mut addrs) = addr.to_socket_addrs() {
            if let Some(sock) = addrs.next() {
                if let Ok(host) = dns_lookup::lookup_addr(&sock.ip()) {
                    if host != ip.to_string() {
                        return Some(host.trim_end_matches('.').to_string());
                    }
                }
            }
        }
        None
    })
    .await
    .ok()
    .flatten()
}

// Lightweight reverse DNS without extra dep - use std only
mod dns_lookup {
    use std::io;
    use std::net::IpAddr;

    pub fn lookup_addr(ip: &IpAddr) -> io::Result<String> {
        #[cfg(target_os = "linux")]
        {
            let output = std::process::Command::new("getent")
                .args(["hosts", &ip.to_string()])
                .output()?;
            if output.status.success() {
                let line = String::from_utf8_lossy(&output.stdout);
                if let Some(host) = line.split_whitespace().nth(1) {
                    return Ok(host.to_string());
                }
            }
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no reverse dns",
        ))
    }
}
