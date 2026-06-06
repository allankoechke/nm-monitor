use futures::stream::{self, StreamExt};
use ipnetwork::IpNetwork;
use std::net::{IpAddr, Ipv4Addr};
use surge_ping::{Client, Config, PingIdentifier, PingSequence};
use tracing::debug;

pub async fn ping_host(addr: IpAddr, timeout_ms: u64) -> Result<bool, surge_ping::SurgeError> {
    let config = Config::default();
    let client = Client::new(&config)?;
    let payload = [0; 16];
    let mut pinger = client.pinger(addr, PingIdentifier(42)).await;
    pinger.timeout(std::time::Duration::from_millis(timeout_ms));
    match pinger.ping(PingSequence(0), &payload).await {
        Ok(_) => Ok(true),
        Err(_) => Ok(false),
    }
}

/// ICMP ping all hosts on the gateway subnet to populate the kernel ARP table.
pub async fn ping_subnet(
    subnet: &IpNetwork,
    exclude: Option<Ipv4Addr>,
    timeout_ms: u64,
    concurrency: usize,
) {
    let hosts: Vec<Ipv4Addr> = crate::arp::subnet_host_ips(subnet, exclude);
    debug!(count = hosts.len(), "pinging subnet hosts");

    stream::iter(hosts)
        .map(|ip| async move {
            let _ = ping_host(IpAddr::V4(ip), timeout_ms).await;
        })
        .buffer_unordered(concurrency)
        .collect::<Vec<_>>()
        .await;
}
