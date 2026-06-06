use etherparse::{ArpOperation, ArpPacket};
use mac_address::MacAddress;
use nm_core::device::DeviceSnapshot;
use nm_classify::oui::lookup_vendor;
use pcap::{Capture, Device as PcapDevice};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use tokio::sync::mpsc;
use tracing::{error, info, warn};

#[derive(Debug, Clone)]
pub struct PassiveObservation {
    pub snapshot: DeviceSnapshot,
    pub dhcp_hostname: Option<String>,
}

pub struct PassiveCapture {
    interface: String,
}

impl PassiveCapture {
    pub fn new(interface: &str) -> Self {
        Self {
            interface: interface.to_string(),
        }
    }

    pub fn spawn(self, tx: mpsc::Sender<PassiveObservation>) -> tokio::task::JoinHandle<()> {
        tokio::task::spawn_blocking(move || {
            if let Err(e) = self.run(tx) {
                error!("passive capture failed: {e}");
            }
        })
    }

    fn run(&self, tx: mpsc::Sender<PassiveObservation>) -> Result<(), String> {
        let device = PcapDevice::list()
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|d| d.name == self.interface)
            .ok_or_else(|| format!("pcap device {} not found", self.interface))?;

        let mut cap = Capture::from_device(device)
            .map_err(|e| e.to_string())?
            .promisc(true)
            .snaplen(65535)
            .timeout(1000)
            .open()
            .map_err(|e| e.to_string())?;

        if let Err(e) = cap.filter("arp or udp port 67 or udp port 68", true) {
            warn!("could not set pcap filter: {e}");
        }

        info!(interface = %self.interface, "passive capture started");
        let mut seen: HashMap<MacAddress, PassiveObservation> = HashMap::new();

        loop {
            match cap.next_packet() {
                Ok(packet) => {
                    if let Some(obs) = parse_packet(packet.data) {
                        let is_new = !seen.contains_key(&obs.snapshot.mac);
                        seen.insert(obs.snapshot.mac, obs.clone());
                        if is_new {
                            let _ = tx.blocking_send(obs);
                        }
                    }
                }
                Err(pcap::Error::TimeoutExpired) => continue,
                Err(e) => return Err(e.to_string()),
            }
        }
    }
}

fn parse_packet(data: &[u8]) -> Option<PassiveObservation> {
    if data.len() >= 14 {
        if let Ok(arp) = ArpPacket::from_slice(&data[14..]) {
            if arp.operation == ArpOperation::REQUEST || arp.operation == ArpOperation::REPLY {
                let hw = arp.sender_hw_addr();
                if hw.len() != 6 {
                    return None;
                }
                let mac = MacAddress::new([hw[0], hw[1], hw[2], hw[3], hw[4], hw[5]]);
                let proto = arp.sender_protocol_addr();
                if proto.len() != 4 {
                    return None;
                }
                let ip = IpAddr::V4(Ipv4Addr::new(proto[0], proto[1], proto[2], proto[3]));
                if let IpAddr::V4(v4) = ip {
                    if !v4.is_unspecified() && !v4.is_broadcast() {
                        return Some(PassiveObservation {
                            snapshot: DeviceSnapshot {
                                mac,
                                ip: Some(ip),
                                hostname: None,
                                vendor: lookup_vendor(&mac),
                            },
                            dhcp_hostname: None,
                        });
                    }
                }
            }
        }
    }

    parse_dhcp_hostname(data)
}

fn parse_dhcp_hostname(data: &[u8]) -> Option<PassiveObservation> {
    if data.len() < 42 {
        return None;
    }
    let ethertype = u16::from_be_bytes([data[12], data[13]]);
    if ethertype != 0x0800 {
        return None;
    }
    let ihl = (data[14] & 0x0f) as usize * 4;
    if data.len() < 14 + ihl + 8 {
        return None;
    }
    let ip_start = 14;
    let proto = data[ip_start + 9];
    if proto != 17 {
        return None;
    }
    let udp_start = ip_start + ihl;
    let src_port = u16::from_be_bytes([data[udp_start], data[udp_start + 1]]);
    let dst_port = u16::from_be_bytes([data[udp_start + 2], data[udp_start + 3]]);
    if !((src_port == 68 && dst_port == 67) || (src_port == 67 && dst_port == 68)) {
        return None;
    }
    let bootp_start = udp_start + 8;
    if data.len() < bootp_start + 240 {
        return None;
    }
    let chaddr = &data[bootp_start + 28..bootp_start + 34];
    let mac = MacAddress::new([
        chaddr[0], chaddr[1], chaddr[2], chaddr[3], chaddr[4], chaddr[5],
    ]);
    let mut opt_start = bootp_start + 236;
    if opt_start + 4 > data.len() {
        return None;
    }
    if &data[opt_start..opt_start + 4] != [99, 130, 83, 99] {
        return None;
    }
    opt_start += 4;
    let mut hostname = None;
    while opt_start < data.len() {
        let code = data[opt_start];
        if code == 255 {
            break;
        }
        if code == 0 {
            opt_start += 1;
            continue;
        }
        if opt_start + 1 >= data.len() {
            break;
        }
        let len = data[opt_start + 1] as usize;
        if opt_start + 2 + len > data.len() {
            break;
        }
        if code == 12 {
            hostname = Some(
                String::from_utf8_lossy(&data[opt_start + 2..opt_start + 2 + len]).to_string(),
            );
        }
        opt_start += 2 + len;
    }
    Some(PassiveObservation {
        snapshot: DeviceSnapshot {
            mac,
            ip: None,
            hostname: hostname.clone(),
            vendor: lookup_vendor(&mac),
        },
        dhcp_hostname: hostname,
    })
}
