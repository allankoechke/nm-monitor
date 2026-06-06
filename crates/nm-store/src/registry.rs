use crate::error::StoreError;
use crate::store::Store;
use chrono::Utc;
use mac_address::MacAddress;
use nm_core::{
    device::{Device, DeviceKind, DeviceSnapshot, OsHint},
    event::{EventKind, EventRecord},
};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub enum RegistryEvent {
    DeviceJoined {
        device: Device,
        identity_name: Option<String>,
    },
    DeviceLeft {
        device: Device,
        identity_name: Option<String>,
    },
    DeviceReturned {
        device: Device,
        identity_name: Option<String>,
    },
    IpChanged {
        device: Device,
        old_ip: Option<std::net::IpAddr>,
    },
}

pub struct DeviceRegistry {
    store: Arc<Store>,
    agent_name: String,
}

impl DeviceRegistry {
    pub fn new(store: Arc<Store>, agent_name: String) -> Self {
        Self { store, agent_name }
    }

    pub fn set_agent_name(&mut self, name: String) {
        self.agent_name = name;
    }

    pub fn agent_name(&self) -> &str {
        &self.agent_name
    }

    pub fn process_sweep(
        &self,
        snapshots: &[DeviceSnapshot],
        network_name: Option<&str>,
    ) -> Result<Vec<RegistryEvent>, StoreError> {
        let now = Utc::now();
        let mut seen: HashMap<MacAddress, DeviceSnapshot> = HashMap::new();
        for snap in snapshots {
            seen.insert(snap.mac, snap.clone());
        }

        let existing = self.store.list_devices()?;
        let mut events = Vec::new();

        for snap in snapshots {
            if let Some(mut device) = self.store.get_device(&snap.mac)? {
                let was_online = device.online;
                let old_ip = device.current_ip;
                device.current_ip = snap.ip.or(device.current_ip);
                if snap.hostname.is_some() {
                    device.hostname = snap.hostname.clone();
                }
                if snap.vendor.is_some() {
                    device.vendor = snap.vendor.clone();
                }
                device.last_seen = now;
                device.online = true;
                self.store.upsert_device(&device)?;

                let identity_name = device
                    .identity_id
                    .as_ref()
                    .and_then(|id| self.store.get_identity(id).ok().flatten())
                    .map(|i| i.display_name);

                if !was_online {
                    let kind = if device.first_seen == device.last_seen {
                        EventKind::DeviceJoined
                    } else {
                        EventKind::DeviceReturned
                    };
                    let evt = if kind == EventKind::DeviceJoined {
                        RegistryEvent::DeviceJoined {
                            device: device.clone(),
                            identity_name: identity_name.clone(),
                        }
                    } else {
                        RegistryEvent::DeviceReturned {
                            device: device.clone(),
                            identity_name: identity_name.clone(),
                        }
                    };
                    events.push(evt);
                    self.record_device_event(&device, kind, network_name)?;
                }

                if old_ip != device.current_ip && device.current_ip.is_some() {
                    events.push(RegistryEvent::IpChanged {
                        device: device.clone(),
                        old_ip,
                    });
                    self.record_event(
                        EventKind::IpChanged,
                        network_name,
                        Some(&device),
                        format!(
                            "IP changed from {} to {}",
                            old_ip
                                .map(|i| i.to_string())
                                .unwrap_or_else(|| "none".into()),
                            device
                                .current_ip
                                .map(|i| i.to_string())
                                .unwrap_or_else(|| "none".into())
                        ),
                        None,
                    )?;
                }
            } else {
                let device = Device {
                    mac: snap.mac,
                    current_ip: snap.ip,
                    hostname: snap.hostname.clone(),
                    vendor: snap.vendor.clone(),
                    kind: DeviceKind::Unknown,
                    os_hint: OsHint::Unknown,
                    identity_id: None,
                    user_label: None,
                    first_seen: now,
                    last_seen: now,
                    online: true,
                    open_ports: Vec::new(),
                    mdns_services: Vec::new(),
                    confidence: 0.0,
                    inference_source: None,
                    do_not_scan: false,
                };
                self.store.upsert_device(&device)?;
                events.push(RegistryEvent::DeviceJoined {
                    device: device.clone(),
                    identity_name: None,
                });
                self.record_device_event(&device, EventKind::DeviceJoined, network_name)?;
            }
        }

        for device in existing {
            if device.online && !seen.contains_key(&device.mac) {
                let mut offline = device.clone();
                offline.online = false;
                self.store.upsert_device(&offline)?;
                let identity_name = offline
                    .identity_id
                    .as_ref()
                    .and_then(|id| self.store.get_identity(id).ok().flatten())
                    .map(|i| i.display_name);
                events.push(RegistryEvent::DeviceLeft {
                    device: offline.clone(),
                    identity_name,
                });
                self.record_device_event(&offline, EventKind::DeviceLeft, network_name)?;
            }
        }

        Ok(events)
    }

    pub fn mark_stale_offline(
        &self,
        timeout_secs: u64,
        network_name: Option<&str>,
    ) -> Result<Vec<RegistryEvent>, StoreError> {
        let now = Utc::now();
        let mut events = Vec::new();
        for mut device in self.store.list_devices()? {
            if device.online {
                let elapsed = now.signed_duration_since(device.last_seen);
                if elapsed.num_seconds() > timeout_secs as i64 {
                    device.online = false;
                    self.store.upsert_device(&device)?;
                    let identity_name = device
                        .identity_id
                        .as_ref()
                        .and_then(|id| self.store.get_identity(id).ok().flatten())
                        .map(|i| i.display_name);
                    events.push(RegistryEvent::DeviceLeft {
                        device: device.clone(),
                        identity_name,
                    });
                    self.record_device_event(&device, EventKind::DeviceLeft, network_name)?;
                }
            }
        }
        Ok(events)
    }

    fn record_device_event(
        &self,
        device: &Device,
        kind: EventKind,
        network_name: Option<&str>,
    ) -> Result<(), StoreError> {
        let message = match kind {
            EventKind::DeviceJoined => format!("{} joined", device.mac),
            EventKind::DeviceLeft => format!("{} left", device.mac),
            EventKind::DeviceReturned => format!("{} returned", device.mac),
            _ => device.mac.to_string(),
        };
        let _ = self.record_event(kind, network_name, Some(device), message, None)?;
        Ok(())
    }

    pub fn record_event(
        &self,
        kind: EventKind,
        network_name: Option<&str>,
        device: Option<&Device>,
        message: String,
        details: Option<serde_json::Value>,
    ) -> Result<EventRecord, StoreError> {
        let event = EventRecord {
            id: Uuid::new_v4(),
            kind,
            timestamp: Utc::now(),
            agent_name: self.agent_name.clone(),
            network_name: network_name.map(str::to_string),
            device_mac: device.map(|d| d.mac),
            device_ip: device.and_then(|d| d.current_ip),
            message,
            details,
        };
        self.store.insert_event(&event)?;
        Ok(event)
    }
}
