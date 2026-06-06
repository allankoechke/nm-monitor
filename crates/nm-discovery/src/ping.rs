use std::net::IpAddr;
use surge_ping::{Client, Config, PingIdentifier, PingSequence};

pub async fn ping_host(addr: IpAddr, timeout_ms: u64) -> Result<bool, surge_ping::SurgeError> {
    let config = Config::default();
    let client = Client::new(&config)?;
    let payload = [0; 16];
    let mut pinger = client.pinger(addr, PingIdentifier(42)).await?;
    pinger.timeout(std::time::Duration::from_millis(timeout_ms));
    match pinger.ping(PingSequence(0), &payload).await {
        Ok(_) => Ok(true),
        Err(_) => Ok(false),
    }
}
