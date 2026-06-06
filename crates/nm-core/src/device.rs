use chrono::{DateTime, Utc};
use mac_address::MacAddress;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceKind {
    Router,
    Mobile,
    Desktop,
    IoT,
    Unknown,
}

impl Default for DeviceKind {
    fn default() -> Self {
        Self::Unknown
    }
}

impl std::fmt::Display for DeviceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Router => write!(f, "router"),
            Self::Mobile => write!(f, "mobile"),
            Self::Desktop => write!(f, "desktop"),
            Self::IoT => write!(f, "iot"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OsHint {
    Android,
    Ios,
    Linux,
    MacOS,
    Windows,
    Unknown,
}

impl Default for OsHint {
    fn default() -> Self {
        Self::Unknown
    }
}

impl std::fmt::Display for OsHint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Android => write!(f, "android"),
            Self::Ios => write!(f, "ios"),
            Self::Linux => write!(f, "linux"),
            Self::MacOS => write!(f, "macos"),
            Self::Windows => write!(f, "windows"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
    pub mac: MacAddress,
    pub current_ip: Option<IpAddr>,
    pub hostname: Option<String>,
    pub vendor: Option<String>,
    pub kind: DeviceKind,
    pub os_hint: OsHint,
    pub identity_id: Option<Uuid>,
    pub user_label: Option<String>,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub online: bool,
    pub open_ports: Vec<u16>,
    pub mdns_services: Vec<String>,
    pub confidence: f32,
    pub inference_source: Option<String>,
    pub do_not_scan: bool,
}

impl Device {
    pub fn display_name(&self, identity_name: Option<&str>) -> String {
        if let Some(label) = &self.user_label {
            return label.clone();
        }
        if let Some(name) = identity_name {
            return name.to_string();
        }
        if let Some(host) = &self.hostname {
            return host.clone();
        }
        if let Some(vendor) = &self.vendor {
            return format!("{vendor} device");
        }
        format!("{}", self.mac)
    }

    pub fn kind_label(&self) -> String {
        match (self.kind, self.os_hint) {
            (DeviceKind::Mobile, OsHint::Android) => "Android phone".into(),
            (DeviceKind::Mobile, OsHint::Ios) => "iPhone".into(),
            (DeviceKind::Mobile, _) => "mobile device".into(),
            (DeviceKind::Desktop, OsHint::Linux) => "Linux desktop".into(),
            (DeviceKind::Desktop, OsHint::MacOS) => "Mac".into(),
            (DeviceKind::Desktop, OsHint::Windows) => "Windows PC".into(),
            (DeviceKind::Desktop, _) => "desktop".into(),
            (DeviceKind::Router, _,) => "router".into(),
            (DeviceKind::IoT, _) => "IoT device".into(),
            (DeviceKind::Unknown, _) => "device".into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DeviceSnapshot {
    pub mac: MacAddress,
    pub ip: Option<IpAddr>,
    pub hostname: Option<String>,
    pub vendor: Option<String>,
}
