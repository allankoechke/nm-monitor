use crate::event::EventKind;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationPayload {
    pub agent_name: String,
    pub network_name: Option<String>,
    pub kind: EventKind,
    pub title: String,
    pub body: String,
    pub timestamp: DateTime<Utc>,
    pub device_name: Option<String>,
    pub device_ip: Option<IpAddr>,
    pub gateway: Option<IpAddr>,
}

pub fn format_notification(payload: &NotificationPayload) -> (String, String) {
    let prefix = format!("[{}]", payload.agent_name);
    let title = format!("{prefix} {}", payload.title);
    let body = payload.body.clone();
    (title, body)
}

pub fn network_down_title(network_name: Option<&str>) -> String {
    match network_name {
        Some(ssid) => format!("\"{ssid}\" is down"),
        None => "Network is down".into(),
    }
}

pub fn network_down_body(gateway: Option<IpAddr>, network_name: Option<&str>) -> String {
    let gw = gateway
        .map(|g| format!("gateway {g} unreachable"))
        .unwrap_or_else(|| "link unreachable".into());
    match network_name {
        Some(ssid) => format!("WiFi \"{ssid}\" — {gw}"),
        None => gw,
    }
}

pub fn network_restored_title(network_name: Option<&str>) -> String {
    match network_name {
        Some(ssid) => format!("\"{ssid}\" restored"),
        None => "Network restored".into(),
    }
}

pub fn network_restored_body(network_name: Option<&str>) -> String {
    match network_name {
        Some(ssid) => format!("WiFi \"{ssid}\" is back online"),
        None => "Network connectivity restored".into(),
    }
}

pub fn device_joined_body(
    device_name: &str,
    kind_label: &str,
    ip: Option<IpAddr>,
    network_name: Option<&str>,
) -> String {
    let ip_part = ip.map(|i| format!(", {i}")).unwrap_or_default();
    match network_name {
        Some(ssid) => format!("{device_name}'s {kind_label}{ip_part} joined \"{ssid}\""),
        None => format!("{device_name}'s {kind_label}{ip_part} joined the network"),
    }
}

pub fn device_left_body(device_name: &str, kind_label: &str, network_name: Option<&str>) -> String {
    match network_name {
        Some(ssid) => format!("{device_name}'s {kind_label} left \"{ssid}\""),
        None => format!("{device_name}'s {kind_label} left the network"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_down_omits_ssid_when_unknown() {
        assert_eq!(network_down_title(None), "Network is down");
        assert_eq!(
            network_down_body(Some("192.168.1.1".parse().unwrap()), None),
            "gateway 192.168.1.1 unreachable"
        );
    }

    #[test]
    fn network_down_includes_ssid_when_known() {
        assert_eq!(network_down_title(Some("HomeWiFi")), "\"HomeWiFi\" is down");
    }

    #[test]
    fn agent_prefix_in_notification() {
        let payload = NotificationPayload {
            agent_name: "Home Pi".into(),
            network_name: Some("HomeWiFi".into()),
            kind: EventKind::NetworkRestored,
            title: network_restored_title(Some("HomeWiFi")),
            body: network_restored_body(Some("HomeWiFi")),
            timestamp: Utc::now(),
            device_name: None,
            device_ip: None,
            gateway: None,
        };
        let (title, _) = format_notification(&payload);
        assert!(title.starts_with("[Home Pi]"));
    }
}
