use ipnetwork::IpNetwork;
use mac_address::MacAddress;
use pnet::datalink::{self, Channel::Ethernet, MacAddr};
use pnet::packet::arp::{ArpHardwareTypes, ArpOperations, ArpPacket, MutableArpPacket};
use pnet::packet::ethernet::{EtherTypes, EthernetPacket, MutableEthernetPacket};
use pnet::packet::Packet;
use pnet::util;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::str::FromStr;
use std::time::{Duration, Instant};
use thiserror::Error;
use tracing::{debug, warn};

#[derive(Debug, Error)]
pub enum ArpError {
    #[error("arp error: {0}")]
    Failed(String),
}

pub fn subnet_host_ips(subnet: &IpNetwork, exclude: Option<Ipv4Addr>) -> Vec<Ipv4Addr> {
    match subnet {
        IpNetwork::V4(v4) => v4
            .iter()
            .filter(|ip| {
                if ip.is_broadcast() || ip.is_unspecified() {
                    return false;
                }
                if let Some(ex) = exclude {
                    if *ip == ex {
                        return false;
                    }
                }
                // Skip network address (x.x.x.0)
                if ip.octets()[3] == 0 {
                    return false;
                }
                true
            })
            .collect(),
        IpNetwork::V6(_) => Vec::new(),
    }
}

/// Read the kernel ARP cache for devices on `interface` within `subnet`.
pub fn read_proc_arp(
    interface: &str,
    subnet: &IpNetwork,
) -> Result<HashMap<IpAddr, MacAddress>, ArpError> {
    let content = std::fs::read_to_string("/proc/net/arp")
        .map_err(|e| ArpError::Failed(format!("read /proc/net/arp: {e}")))?;
    let mut results = HashMap::new();

    for line in content.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 6 {
            continue;
        }
        let ip: Ipv4Addr = match parts[0].parse() {
            Ok(ip) => ip,
            Err(_) => continue,
        };
        let flags = match u32::from_str_radix(parts[2].trim_start_matches("0x"), 16) {
            Ok(f) => f,
            Err(_) => continue,
        };
        // ATF_COM — complete entry
        if flags & 0x02 == 0 {
            continue;
        }
        if parts[3] == "00:00:00:00:00:00" {
            continue;
        }
        if parts[5] != interface {
            continue;
        }
        let addr = IpAddr::V4(ip);
        if !subnet.contains(addr) {
            continue;
        }
        if let Ok(mac) = MacAddress::from_str(parts[3]) {
            results.insert(addr, mac);
        }
    }

    debug!(
        interface,
        count = results.len(),
        "read /proc/net/arp"
    );
    Ok(results)
}

pub fn arp_sweep(
    interface_name: &str,
    subnet: &IpNetwork,
    local_ip: Option<Ipv4Addr>,
) -> Result<HashMap<IpAddr, MacAddress>, ArpError> {
    let interfaces = datalink::interfaces();
    let interface = interfaces
        .iter()
        .find(|i| i.name == interface_name)
        .ok_or_else(|| ArpError::Failed(format!("interface {interface_name} not found")))?;

    let src_mac = interface
        .mac
        .ok_or_else(|| ArpError::Failed("interface has no MAC".into()))?;
    let src_ip = local_ip.or_else(|| {
        interface.ips.iter().find_map(|n| match n {
            pnet::ipnetwork::IpNetwork::V4(v4) => {
                let ip = v4.ip();
                if subnet.contains(IpAddr::V4(ip)) {
                    Some(ip)
                } else {
                    None
                }
            }
            _ => None,
        })
    }).ok_or_else(|| ArpError::Failed("interface has no IPv4 on subnet".into()))?;

    let targets = subnet_host_ips(subnet, Some(src_ip));

    let (mut tx, mut rx) = match datalink::channel(interface, Default::default()) {
        Ok(Ethernet(tx, rx)) => (tx, rx),
        Ok(_) => return Err(ArpError::Failed("unsupported channel type".into())),
        Err(e) => return Err(ArpError::Failed(e.to_string())),
    };

    let mut results = HashMap::new();
    const BATCH: usize = 32;
    const BATCH_WAIT_MS: u64 = 150;
    const TOTAL_WAIT_SECS: u64 = 4;

    let deadline = Instant::now() + Duration::from_secs(TOTAL_WAIT_SECS);

    for chunk in targets.chunks(BATCH) {
        for target_ip in chunk {
            if let Err(e) = send_arp_request(&mut tx, &src_mac, src_ip, *target_ip) {
                warn!(%target_ip, error = %e, "failed to send ARP request");
            }
        }

        let batch_deadline = Instant::now() + Duration::from_millis(BATCH_WAIT_MS);
        while Instant::now() < batch_deadline && Instant::now() < deadline {
            match rx.next() {
                Ok(packet) => {
                    if let Some((ip, mac)) = parse_arp_packet(packet, src_ip) {
                        results.insert(IpAddr::V4(ip), mac);
                    }
                }
                Err(_) => {
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        }
    }

    // Drain remaining replies until deadline
    while Instant::now() < deadline {
        match rx.next() {
            Ok(packet) => {
                if let Some((ip, mac)) = parse_arp_packet(packet, src_ip) {
                    results.insert(IpAddr::V4(ip), mac);
                }
            }
            Err(_) => {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }

    debug!(
        interface = interface_name,
        targets = targets.len(),
        replies = results.len(),
        "ARP sweep finished"
    );

    Ok(results)
}

fn send_arp_request(
    tx: &mut Box<dyn datalink::DataLinkSender>,
    src_mac: &MacAddr,
    src_ip: Ipv4Addr,
    target_ip: Ipv4Addr,
) -> Result<(), ArpError> {
    let mut ethernet_buffer = [0u8; 42];
    let mut arp_buffer = [0u8; 28];

    let mut arp_packet = MutableArpPacket::new(&mut arp_buffer)
        .ok_or_else(|| ArpError::Failed("arp packet".into()))?;
    arp_packet.set_hardware_type(ArpHardwareTypes::Ethernet);
    arp_packet.set_protocol_type(EtherTypes::Ipv4);
    arp_packet.set_hw_addr_len(6);
    arp_packet.set_proto_addr_len(4);
    arp_packet.set_operation(ArpOperations::Request);
    arp_packet.set_sender_hw_addr(*src_mac);
    arp_packet.set_sender_proto_addr(src_ip);
    arp_packet.set_target_hw_addr(MacAddr::zero());
    arp_packet.set_target_proto_addr(target_ip);

    let mut ethernet_packet = MutableEthernetPacket::new(&mut ethernet_buffer)
        .ok_or_else(|| ArpError::Failed("ethernet packet".into()))?;
    ethernet_packet.set_destination(util::MacAddr(0xff, 0xff, 0xff, 0xff, 0xff, 0xff));
    ethernet_packet.set_source(*src_mac);
    ethernet_packet.set_ethertype(EtherTypes::Arp);
    ethernet_packet.set_payload(arp_packet.packet());

    tx.build_and_send(1, ethernet_packet.packet().len(), &mut |new_packet| {
        new_packet.copy_from_slice(ethernet_packet.packet());
    });

    Ok(())
}

fn parse_arp_packet(packet: &[u8], our_ip: Ipv4Addr) -> Option<(Ipv4Addr, MacAddress)> {
    let ethernet = EthernetPacket::new(packet)?;
    if ethernet.get_ethertype() != EtherTypes::Arp {
        return None;
    }
    let arp = ArpPacket::new(ethernet.payload())?;
    let sender_ip = arp.get_sender_proto_addr();
    if sender_ip == our_ip || sender_ip.is_unspecified() || sender_ip.is_broadcast() {
        return None;
    }

    match arp.get_operation() {
        ArpOperations::Reply => {}
        ArpOperations::Request => {
            // Gratuitous / peer ARP announcements on the LAN
            if arp.get_target_proto_addr() != our_ip {
                return None;
            }
        }
        _ => return None,
    }

    let hw = arp.get_sender_hw_addr();
    let mac = MacAddress::new([hw.0, hw.1, hw.2, hw.3, hw.4, hw.5]);
    Some((sender_ip, mac))
}
