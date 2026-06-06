use mac_address::MacAddress;
use nm_core::device::{DeviceKind, OsHint};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default)]
pub struct ClassificationInput {
    pub mac: MacAddress,
    pub vendor: Option<String>,
    pub hostname: Option<String>,
    pub open_ports: Vec<u16>,
    pub mdns_services: Vec<String>,
    pub dhcp_hostname: Option<String>,
    pub is_gateway: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassificationResult {
    pub kind: DeviceKind,
    pub os_hint: OsHint,
    pub confidence: f32,
    pub inference_source: String,
}

pub struct DeviceClassifier;

impl DeviceClassifier {
    pub fn classify(input: &ClassificationInput) -> ClassificationResult {
        let mut kind = DeviceKind::Unknown;
        let mut os_hint = OsHint::Unknown;
        let mut confidence: f32 = 0.0;
        let mut sources = Vec::new();

        if input.is_gateway {
            kind = DeviceKind::Router;
            confidence = 0.9;
            sources.push("gateway_ip");
        }

        if let Some(vendor) = &input.vendor {
            let v = vendor.to_lowercase();
            if v.contains("apple") {
                kind = DeviceKind::Mobile;
                os_hint = OsHint::Ios;
                confidence = confidence.max(0.7);
                sources.push("oui_apple");
            } else if v.contains("samsung") || v.contains("xiaomi") || v.contains("google") {
                kind = DeviceKind::Mobile;
                os_hint = OsHint::Android;
                confidence = confidence.max(0.65);
                sources.push("oui_android_vendor");
            } else if v.contains("raspberry") {
                kind = DeviceKind::Desktop;
                os_hint = OsHint::Linux;
                confidence = confidence.max(0.8);
                sources.push("oui_raspberry");
            } else if v.contains("intel") || v.contains("microsoft") {
                kind = DeviceKind::Desktop;
                confidence = confidence.max(0.5);
                sources.push("oui_desktop_vendor");
                if v.contains("microsoft") {
                    os_hint = OsHint::Windows;
                }
            } else if v.contains("cisco") || v.contains("tp-link") || v.contains("asus") || v.contains("linksys")
            {
                kind = DeviceKind::Router;
                confidence = confidence.max(0.75);
                sources.push("oui_router_vendor");
            }
        }

        for service in &input.mdns_services {
            let s = service.to_lowercase();
            if s.contains("airplay") || s.contains("apple") {
                kind = DeviceKind::Mobile;
                os_hint = OsHint::Ios;
                confidence = confidence.max(0.85);
                sources.push("mdns_airplay");
            } else if s.contains("androidtv") || s.contains("googlecast") {
                kind = DeviceKind::Mobile;
                os_hint = OsHint::Android;
                confidence = confidence.max(0.8);
                sources.push("mdns_android");
            } else if s.contains("smb") || s.contains("ssh") {
                kind = DeviceKind::Desktop;
                confidence = confidence.max(0.6);
                sources.push("mdns_desktop_service");
            }
        }

        for port in &input.open_ports {
            match port {
                22 => {
                    kind = DeviceKind::Desktop;
                    os_hint = OsHint::Linux;
                    confidence = confidence.max(0.55);
                    sources.push("port_22");
                }
                445 | 139 => {
                    kind = DeviceKind::Desktop;
                    os_hint = OsHint::Windows;
                    confidence = confidence.max(0.6);
                    sources.push("port_smb");
                }
                548 | 631 => {
                    kind = DeviceKind::Desktop;
                    os_hint = OsHint::MacOS;
                    confidence = confidence.max(0.55);
                    sources.push("port_mac");
                }
                62078 => {
                    kind = DeviceKind::Mobile;
                    os_hint = OsHint::Ios;
                    confidence = confidence.max(0.9);
                    sources.push("port_ios_sync");
                }
                80 | 443 | 8080 if input.is_gateway => {
                    kind = DeviceKind::Router;
                    confidence = confidence.max(0.7);
                    sources.push("port_router_web");
                }
                _ => {}
            }
        }

        let host = input
            .hostname
            .as_deref()
            .or(input.dhcp_hostname.as_deref())
            .unwrap_or("")
            .to_lowercase();
        if host.contains("iphone") || host.contains("ipad") {
            kind = DeviceKind::Mobile;
            os_hint = OsHint::Ios;
            confidence = confidence.max(0.85);
            sources.push("hostname_apple");
        } else if host.contains("android") || host.contains("galaxy") || host.contains("pixel") {
            kind = DeviceKind::Mobile;
            os_hint = OsHint::Android;
            confidence = confidence.max(0.85);
            sources.push("hostname_android");
        } else if host.contains("router") || host.contains("gateway") {
            kind = DeviceKind::Router;
            confidence = confidence.max(0.8);
            sources.push("hostname_router");
        }

        ClassificationResult {
            kind,
            os_hint,
            confidence,
            inference_source: if sources.is_empty() {
                "none".into()
            } else {
                sources.join(",")
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn classifies_apple_oui_as_ios() {
        let mac = MacAddress::from_str("AC:DE:48:00:11:22").unwrap();
        let result = DeviceClassifier::classify(&ClassificationInput {
            mac,
            vendor: Some("Apple".into()),
            ..Default::default()
        });
        assert_eq!(result.kind, DeviceKind::Mobile);
        assert_eq!(result.os_hint, OsHint::Ios);
    }

    #[test]
    fn classifies_gateway_as_router() {
        let mac = MacAddress::from_str("00:1D:0F:00:11:22").unwrap();
        let result = DeviceClassifier::classify(&ClassificationInput {
            mac,
            vendor: Some("TP-Link".into()),
            is_gateway: true,
            open_ports: vec![80, 443],
            ..Default::default()
        });
        assert_eq!(result.kind, DeviceKind::Router);
    }
}
