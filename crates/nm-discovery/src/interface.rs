use ipnetwork::IpNetwork;
use pnet::datalink;
use std::net::{IpAddr, Ipv4Addr};
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

/// Default route from the routing table: (interface name, gateway IPv4).
pub fn detect_default_route() -> Option<(String, Ipv4Addr)> {
    let content = std::fs::read_to_string("/proc/net/route").ok()?;
    for line in content.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 || parts[1] != "00000000" {
            continue;
        }
        let iface = parts[0].to_string();
        let gw_hex = parts[2];
        if gw_hex.len() != 8 {
            continue;
        }
        let bytes = u32::from_str_radix(gw_hex, 16).ok()?;
        let ip = Ipv4Addr::from(bytes.to_le_bytes());
        if !ip.is_unspecified() {
            return Some((iface, ip));
        }
    }
    None
}

pub fn detect_network(interface: &str, gateway: &str) -> Result<NetworkInfo, InterfaceError> {
    let interfaces = datalink::interfaces();
    let default_route = detect_default_route();

    let (iface_name, gateway_ip) = if interface == "auto" {
        if let Some((iface, gw)) = default_route.clone() {
            let gw_ip = if gateway == "auto" {
                IpAddr::V4(gw)
            } else {
                gateway
                    .parse()
                    .map_err(|_| InterfaceError::Other(format!("invalid gateway: {gateway}")))?
            };
            (iface, Some(gw_ip))
        } else {
            let iface = interfaces
                .iter()
                .find(|i| !i.is_loopback() && i.is_up() && !i.ips.is_empty())
                .ok_or(InterfaceError::NotFound)?;
            let gw_ip = if gateway == "auto" {
                None
            } else {
                gateway.parse().ok()
            };
            (iface.name.clone(), gw_ip)
        }
    } else {
        let gw_ip = if gateway == "auto" {
            default_route
                .filter(|(iface, _)| iface == interface)
                .map(|(_, gw)| IpAddr::V4(gw))
                .or_else(|| {
                    interfaces
                        .iter()
                        .find(|i| i.name == interface)
                        .and_then(|iface| {
                            iface.ips.iter().find_map(|n| {
                                if let pnet::ipnetwork::IpNetwork::V4(v4) = n {
                                    IpNetwork::new(v4.ip().into(), v4.prefix())
                                        .ok()
                                        .and_then(|net| guess_gateway(&net))
                                } else {
                                    None
                                }
                            })
                        })
                })
        } else {
            gateway.parse().ok()
        };
        (interface.to_string(), gw_ip)
    };

    let iface = interfaces
        .iter()
        .find(|i| i.name == iface_name)
        .ok_or_else(|| InterfaceError::Other(format!("interface {iface_name} not found")))?;

    let gateway_v4 = gateway_ip.and_then(|g| match g {
        IpAddr::V4(v4) => Some(v4),
        _ => None,
    });

    let ip_network = iface
        .ips
        .iter()
        .find_map(|n| {
            if let pnet::ipnetwork::IpNetwork::V4(v4) = n {
                let prefix = v4.prefix();
                if prefix > 30 {
                    return None;
                }
                let net = IpNetwork::new(v4.ip().into(), prefix).ok()?;
                if let Some(gw) = gateway_v4 {
                    if net.contains(IpAddr::V4(gw)) {
                        return Some(net);
                    }
                } else {
                    return Some(net);
                }
            }
            None
        })
        .or_else(|| {
            iface.ips.iter().find_map(|n| {
                if let pnet::ipnetwork::IpNetwork::V4(v4) = n {
                    let prefix = v4.prefix();
                    if prefix <= 30 {
                        return IpNetwork::new(v4.ip().into(), prefix).ok();
                    }
                }
                None
            })
        })
        .ok_or_else(|| InterfaceError::Other("no IPv4 subnet on interface".into()))?;

    let local_ip = iface.ips.iter().find_map(|n| match n {
        pnet::ipnetwork::IpNetwork::V4(v4) => {
            let ip = IpAddr::V4(v4.ip());
            if ip_network.contains(ip) {
                Some(ip)
            } else {
                None
            }
        }
        _ => None,
    });

    let gateway_ip = gateway_ip.or_else(|| guess_gateway(&ip_network));

    let mac = iface
        .mac
        .map(|m| mac_address::MacAddress::new([m.0, m.1, m.2, m.3, m.4, m.5]));

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
            Some(IpAddr::V4(Ipv4Addr::new(
                octets[0], octets[1], octets[2], 1,
            )))
        }
        IpNetwork::V6(_) => None,
    }
}
