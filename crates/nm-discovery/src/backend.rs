use crate::arp::{arp_sweep, read_proc_arp};
use crate::interface::{detect_network, NetworkInfo};
use crate::ping::{ping_host, ping_subnet};
use async_trait::async_trait;
use mac_address::MacAddress;
use nm_core::device::DeviceSnapshot;
use nm_classify::oui::lookup_vendor;
use std::collections::HashMap;
use std::net::IpAddr;
use std::time::Duration;
use thiserror::Error;
use tracing::info;

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

        let local_v4 = network.local_ip.and_then(|ip| match ip {
            IpAddr::V4(v4) => Some(v4),
            _ => None,
        });

        let gateway_v4 = network.gateway.and_then(|ip| match ip {
            IpAddr::V4(v4) => Some(v4),
            _ => None,
        });

        info!(
            interface = %network.interface,
            subnet = %network.subnet,
            gateway = ?network.gateway,
            local_ip = ?network.local_ip,
            "scanning LAN behind gateway"
        );

        // 1. Ping sweep — forces the kernel to resolve ARP for live hosts on this subnet
        ping_subnet(&network.subnet, local_v4, 400, 64).await;
        tokio::time::sleep(Duration::from_millis(300)).await;

        let mut merged: HashMap<IpAddr, MacAddress> = HashMap::new();

        // 2. Kernel ARP table (most reliable on WiFi after ping)
        if let Ok(arp_cache) = read_proc_arp(&network.interface, &network.subnet) {
            merged.extend(arp_cache);
        }

        // 3. Active ARP sweep for hosts not yet in cache
        match arp_sweep(&network.interface, &network.subnet, local_v4) {
            Ok(arp_results) => merged.extend(arp_results),
            Err(e) => {
                tracing::warn!(error = %e, "ARP sweep failed, using ARP cache only");
            }
        }

        // 4. Re-read ARP table after sweep
        if let Ok(arp_cache) = read_proc_arp(&network.interface, &network.subnet) {
            merged.extend(arp_cache);
        }

        // 5. Ensure gateway is probed explicitly
        if let Some(gw) = gateway_v4 {
            let _ = ping_host(IpAddr::V4(gw), 1_000).await;
            tokio::time::sleep(Duration::from_millis(100)).await;
            if let Ok(arp_cache) = read_proc_arp(&network.interface, &network.subnet) {
                merged.extend(arp_cache);
            }
        }

        // 6. Always include this host — excluded from ping/ARP probes, not in /proc/net/arp
        if let (Some(local_ip), Some(mac)) = (network.local_ip, network.mac) {
            merged.entry(local_ip).or_insert(mac);
        }

        info!(
            interface = %network.interface,
            subnet = %network.subnet,
            device_count = merged.len(),
            "LAN scan finished"
        );

        let local_hostname = local_host_hostname();

        let mut devices = Vec::new();
        for (ip, mac) in merged {
            let vendor = lookup_vendor(&mac);
            let is_local = network.local_ip == Some(ip);
            let hostname = if is_local {
                match local_hostname.clone() {
                    Some(h) => Some(h),
                    None => reverse_dns(ip).await,
                }
            } else {
                reverse_dns(ip).await
            };
            devices.push(DeviceSnapshot {
                mac,
                ip: Some(ip),
                hostname,
                vendor,
            });
        }

        // Stable order for logging
        devices.sort_by_key(|d| d.ip);

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

fn local_host_hostname() -> Option<String> {
    hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .map(|h| h.trim_end_matches(".local").to_string())
        .filter(|h| !h.is_empty())
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
