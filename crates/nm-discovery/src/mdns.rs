use dashmap::DashMap;
use mdns_sd::{ServiceDaemon, ServiceEvent};
use std::collections::HashSet;
use std::time::Duration;
use tracing::{info, warn};

const SERVICE_TYPES: &[&str] = &[
    "_airplay._tcp.local.",
    "_googlecast._tcp.local.",
    "_androidtvremote2._tcp.local.",
    "_smb._tcp.local.",
    "_ssh._tcp.local.",
    "_http._tcp.local.",
    "_ipp._tcp.local.",
];

#[derive(Debug, Clone, Default)]
pub struct MdnsRegistry {
    inner: DashMap<String, HashSet<String>>,
}

impl MdnsRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn services_for_host(&self, hostname: &str) -> Vec<String> {
        self.inner
            .get(hostname)
            .map(|set| set.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn all_hosts(&self) -> Vec<String> {
        self.inner.iter().map(|e| e.key().clone()).collect()
    }
}

pub fn spawn_mdns_browse(registry: MdnsRegistry) -> tokio::task::JoinHandle<()> {
    tokio::task::spawn_blocking(move || {
        if let Err(e) = run_mdns(registry) {
            warn!("mDNS browse ended: {e}");
        }
    })
}

fn run_mdns(registry: MdnsRegistry) -> Result<(), String> {
    let mdns = ServiceDaemon::new().map_err(|e| e.to_string())?;
    let mut receivers = Vec::new();

    for service_type in SERVICE_TYPES {
        match mdns.browse(service_type) {
            Ok(rx) => receivers.push(rx),
            Err(e) => warn!(service_type, error = %e, "mDNS browse failed for service type"),
        }
    }
    info!(count = receivers.len(), "mDNS browse started");

    loop {
        for rx in &receivers {
            match rx.recv_timeout(Duration::from_millis(500)) {
                Ok(event) => match event {
                    ServiceEvent::ServiceResolved(info) => {
                        let host = info
                            .get_hostname()
                            .trim_end_matches('.')
                            .to_string();
                        let service = info.get_fullname().to_string();
                        registry
                            .inner
                            .entry(host)
                            .or_default()
                            .insert(service);
                    }
                    ServiceEvent::ServiceFound(_, _) => {}
                    _ => {}
                },
                Err(_) => continue,
            }
        }
    }
}
