use ipnetwork::IpNetwork;
use mac_address::MacAddress;
use pnet::datalink::{self, Channel::Ethernet, MacAddr};
use pnet::packet::arp::{ArpHardwareTypes, ArpOperations, ArpPacket, MutableArpPacket};
use pnet::packet::ethernet::{EtherTypes, EthernetPacket, MutableEthernetPacket};
use pnet::packet::Packet;
use pnet::util;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ArpError {
    #[error("arp error: {0}")]
    Failed(String),
}

pub fn arp_sweep(interface_name: &str, subnet: &IpNetwork) -> Result<HashMap<IpAddr, MacAddress>, ArpError> {
    let interfaces = datalink::interfaces();
    let interface = interfaces
        .iter()
        .find(|i| i.name == interface_name)
        .ok_or_else(|| ArpError::Failed(format!("interface {interface_name} not found")))?;

    let src_mac = interface
        .mac
        .ok_or_else(|| ArpError::Failed("interface has no MAC".into()))?;
    let src_ip = interface
        .ips
        .iter()
        .find_map(|n| match n {
            pnet::ipnetwork::IpNetwork::V4(v4) => Some(v4.ip()),
            _ => None,
        })
        .ok_or_else(|| ArpError::Failed("interface has no IPv4".into()))?;

    let (mut tx, mut rx) = match datalink::channel(interface, Default::default()) {
        Ok(Ethernet(tx, rx)) => (tx, rx),
        Ok(_) => return Err(ArpError::Failed("unsupported channel type".into())),
        Err(e) => return Err(ArpError::Failed(e.to_string())),
    };

    let targets: Vec<Ipv4Addr> = match subnet {
        IpNetwork::V4(v4) => v4.iter().take(1024).collect(),
        IpNetwork::V6(_) => Vec::new(),
    };

    for target_ip in &targets {
        if *target_ip == src_ip {
            continue;
        }
        send_arp_request(&mut tx, &src_mac, src_ip, *target_ip)?;
    }

    let mut results = HashMap::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        match rx.next() {
            Ok(packet) => {
                if let Some((ip, mac)) = parse_arp_reply(packet, src_ip) {
                    results.insert(IpAddr::V4(ip), mac);
                }
            }
            Err(_) => break,
        }
    }

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

fn parse_arp_reply(packet: &[u8], our_ip: Ipv4Addr) -> Option<(Ipv4Addr, MacAddress)> {
    let ethernet = EthernetPacket::new(packet)?;
    if ethernet.get_ethertype() != EtherTypes::Arp {
        return None;
    }
    let arp = ArpPacket::new(ethernet.payload())?;
    if arp.get_operation() != ArpOperations::Reply {
        return None;
    }
    let sender_ip = arp.get_sender_proto_addr();
    if sender_ip == our_ip || sender_ip.is_unspecified() || sender_ip.is_broadcast() {
        return None;
    }
    let hw = arp.get_sender_hw_addr();
    let mac = MacAddress::new([hw.0, hw.1, hw.2, hw.3, hw.4, hw.5]);
    Some((sender_ip, mac))
}
