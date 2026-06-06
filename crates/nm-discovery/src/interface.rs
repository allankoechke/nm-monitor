use ipnetwork::IpNetwork;
use pnet::datalink;
use std::net::IpAddr;
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct NetworkInfo {
    pub interface: String,
    pub subnet: IpNetwork,
    pub gateway: Option<IpAddr>,
    pub local_ip: Option<IpAddr>,
    pub mac: Option<mac_address::MacAddress>,
}

#[derive(Debug, Error)]
pub enum InterfaceError {
    #[error("no network interface found")]
    NotFound,
    #[error("interface error: {0}")]
    Other(String),
}

pub fn detect_network(interface: &str, gateway: &str) -> Result<NetworkInfo, InterfaceError> {
    let interfaces = datalink::interfaces();
    let iface = if interface == "auto" {
        interfaces
            .iter()
            .find(|i| !i.is_loopback() && i.is_up() && !i.ips.is_empty())
            .ok_or(InterfaceError::NotFound)?
    } else {
        interfaces
            .iter()
            .find(|i| i.name == interface)
            .ok_or_else(|| InterfaceError::Other(format!("interface {interface} not found")))?
    };

    let ip_network = iface
        .ips
        .iter()
        .find_map(|n| {
            if let pnet::ipnetwork::IpNetwork::V4(v4) = n {
                let prefix = v4.prefix();
                if prefix <= 30 {
                    return IpNetwork::new(v4.ip().into(), prefix).ok();
                }
            }
            None
        })
        .ok_or_else(|| InterfaceError::Other("no IPv4 subnet on interface".into()))?;

    let local_ip = ip_network
        .iter()
        .nth(1)
        .or_else(|| iface.ips.iter().find_map(|n| match n {
            pnet::ipnetwork::IpNetwork::V4(v4) => Some(IpAddr::V4(v4.ip())),
            _ => None,
        }));

    let gateway_ip = if gateway == "auto" {
        detect_default_gateway().or_else(|| guess_gateway(&ip_network))
    } else {
        gateway.parse().ok()
    };

    let mac = iface.mac.map(|m| {
        mac_address::MacAddress::from_bytes([
            m.0, m.1, m.2, m.3, m.4, m.5,
        ])
        .unwrap_or(mac_address::MacAddress::nil())
    });

    Ok(NetworkInfo {
        interface: iface.name.clone(),
        subnet: ip_network,
        gateway: gateway_ip,
        local_ip,
        mac,
    })
}

fn guess_gateway(subnet: &IpNetwork) -> Option<IpAddr> {
    match subnet {
        IpNetwork::V4(v4) => {
            let octets = v4.network().octets();
            Some(IpAddr::V4(std::net::Ipv4Addr::new(
                octets[0], octets[1], octets[2], 1,
            )))
        }
        IpNetwork::V6(_) => None,
    }
}

fn detect_default_gateway() -> Option<IpAddr> {
    let content = std::fs::read_to_string("/proc/net/route").ok()?;
    for line in content.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 3 && parts[1] == "00000000" {
            let gw_hex = parts[2];
            if gw_hex.len() == 8 {
                let bytes = u32::from_str_radix(gw_hex, 16).ok()?;
                let ip = std::net::Ipv4Addr::from(bytes.to_le_bytes());
                if !ip.is_unspecified() {
                    return Some(IpAddr::V4(ip));
                }
            }
        }
    }
    None
}
