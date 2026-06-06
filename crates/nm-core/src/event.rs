use chrono::{DateTime, Utc};
use mac_address::MacAddress;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    NetworkDown,
    NetworkRestored,
    DeviceJoined,
    DeviceLeft,
    DeviceReturned,
    IpChanged,
    KindRefined,
    SpeedTestCompleted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventRecord {
    pub id: Uuid,
    pub kind: EventKind,
    pub timestamp: DateTime<Utc>,
    pub agent_name: String,
    pub network_name: Option<String>,
    pub device_mac: Option<MacAddress>,
    pub device_ip: Option<IpAddr>,
    pub message: String,
    pub details: Option<serde_json::Value>,
}
