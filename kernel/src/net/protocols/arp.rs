//! L1: ARP implementation

use core::net::Ipv4Addr;

use crate::net::protocols::eth;
use crate::net::{Mac, NetDevice};
use crate::net::packet::{Packet, PacketLease, ParseError};

// TODO: put these ETH_* constants somewhere..

/// Number of retries for ARP resolution
const N_RETRIES: usize = 1_000;

/// Time in microseconds to wait before timing out on an ARP reply
const TIMEOUT: u64 = 100_000;

/// Ethernet type for ARP
const ETH_TYPE_ARP: u16 = 0x0806;

/// Ethernet type for IPv4
const ETH_TYPE_IPV4: u16 = 0x0800;

/// Hardware type for Ethernet
const HW_TYPE_ETH: u16 = 1;

/// ARP opcodes
#[repr(u16)]
pub enum Opcode {
    Request = 1,
    Reply   = 2,
}

impl NetDevice {
    /// Resolve the MAC address for `ip` using this device
    pub fn arp(&self, ip: Ipv4Addr) -> Option<Mac> {
        // Get this device's IP
        let this_ip  = self.dhcp_lease.lock().as_ref().unwrap().client_ip;
        let this_mac = self.mac();

        'send_arp: for _retry in 0..N_RETRIES {
            // Allocate and send a new ARP packet
            let mut packet = self.allocate_packet();
            self.build_arp_packet(&mut packet, Opcode::Request,
                                  this_mac, this_ip, Mac::ZERO, ip)?;
            self.send(packet, true);

            let timeout = crate::time::future(TIMEOUT);
            loop {
                // If we timed out, retry
                if cpu::rdtsc() >= timeout { continue 'send_arp; }

                if let Some(packet) = self.recv() {
                    if let Ok(arp) = packet.parse_arp() {
                        if arp.is_valid_reply(ip, this_ip, self.mac()) {
                            return Some(arp.sender_mac);
                        }
                    }

                    // We couldn't handle the packet, discard it
                    self.discard(packet);
                }
            }
        }
        // No response
        None
    }

    /// Discard an ARP packet and attempt to handle it somewhere else in the
    /// network stack
    ///
    /// Returns whether this function handled the packet
    pub fn discard_arp(&self, packet: &mut Option<PacketLease>) {
        let pk = match packet.take() {
            None => return,
            Some(pk) => pk,
        };

        // Attempt to parse the packet as ARP
        let arp = match pk.parse_arp() {
            Ok(arp) => arp,
            _ => {
                // Couldn't parse it as ARP. Put the packet back and return
                *packet = Some(pk);
                return;
            },
        };

        // If we don't have a DHCP lease, there's nothing we can do
        let lease = self.dhcp_lease.lock();
        let lease = match lease.as_ref() {
            Some(lease) => lease,
            None => return,
        };

        let this_ip = lease.client_ip;

        // If this was a request to us, reply to it
        if matches!(arp.opcode, x if x == Opcode::Request as u16)
            && arp.hw_type == HW_TYPE_ETH
            && arp.proto_type == ETH_TYPE_IPV4
            && arp.hw_size == 6
            && arp.proto_size == 4
            && arp.target_ip == this_ip
        {
            let mut packet = self.allocate_packet();
            self.build_arp_packet(
                &mut packet, Opcode::Reply,
                self.mac(), this_ip, arp.sender_mac, arp.sender_ip);
            self.send(packet, true);
        }
    }

    fn build_arp_packet(
        &self,
        packet: &mut Packet,
        opcode: Opcode,
        sender_mac: Mac,
        sender_ip: Ipv4Addr,
        target_mac: Mac,
        target_ip: Ipv4Addr,
    ) -> Option<()> {
        let mut cursor = eth::Builder::new(
                packet.cursor(), &sender_mac, &Mac::BROADCAST)?
            .take_cursor();

        cursor.write_u16(HW_TYPE_ETH)?;
        cursor.write_u16(ETH_TYPE_ARP)?;
        cursor.write_u8(6)?;
        cursor.write_u8(4)?;

        cursor.write_u16(opcode as u16)?;

        cursor.write(&sender_mac.0)?;
        cursor.write(&sender_ip.to_bits().to_be_bytes())?;

        cursor.write(&target_mac.0)?;
        cursor.write(&target_ip.to_bits().to_be_bytes())?;

        Some(())
    }
}

/// A IPv4 ethernet ARP packet
#[derive(Debug)]
pub struct Parsed<'a> {
    pub eth: eth::Parsed<'a>,

    pub hw_type:    u16,
    pub proto_type: u16,
    pub hw_size:    u8,
    pub proto_size: u8,
    pub opcode:     u16,
    pub sender_mac: Mac,
    pub sender_ip:  Ipv4Addr,
    pub target_mac: Mac,
    pub target_ip:  Ipv4Addr,
}

impl<'a> Parsed<'a> {
    /// Returns whether this parsed ARP packet is a valid reply with the
    /// expected parameters
    fn is_valid_reply(&self, sender_ip: Ipv4Addr, target_ip: Ipv4Addr,
                      target_mac: Mac) -> bool {
        self.hw_type == HW_TYPE_ETH
            && self.proto_type == ETH_TYPE_IPV4
            && self.hw_size == 6
            && self.proto_size == 4
            && self.opcode == Opcode::Reply as u16
            && self.sender_ip == sender_ip
            && self.target_ip == target_ip
            && self.target_mac == target_mac
    }
}

impl Packet {
    /// Parse the packet into a `Parsed` ARP structure if it is a valid ARP packet.
    pub fn parse_arp(&self) -> Result<Parsed, ParseError> {
        let eth = self.parse_eth()?; // Assume this already returns Result<eth::Parsed, ParseError>

        if eth.eth_type != ETH_TYPE_ARP {
            return Err(ParseError::InvalidLength); // Or consider a new ParseError::NotArp
        }

        let pl = eth.payload;
        if pl.len() < 28 {
            return Err(ParseError::TruncatedPacket);
        }

        let hw_type = u16::from_be_bytes(pl[0..2].try_into()
            .map_err(|_| ParseError::InvalidWord)?);
        let proto_type = u16::from_be_bytes(pl[2..4].try_into()
            .map_err(|_| ParseError::InvalidWord)?);
        let hw_size = pl[4];
        let proto_size = pl[5];
        let opcode = u16::from_be_bytes(pl[6..8].try_into()
            .map_err(|_| ParseError::InvalidWord)?);

        let sender_mac = Mac(pl[8..14].try_into()
            .map_err(|_| ParseError::InvalidMacAddress)?);
        let sender_ip = Ipv4Addr::from(
            u32::from_be_bytes(pl[14..18].try_into()
                .map_err(|_| ParseError::InvalidDword)?),
        );
        let target_mac = Mac(pl[18..24].try_into()
            .map_err(|_| ParseError::InvalidMacAddress)?);
        let target_ip = Ipv4Addr::from(
            u32::from_be_bytes(pl[24..28].try_into()
                .map_err(|_| ParseError::InvalidDword)?),
        );

        Ok(Parsed {
            eth,
            hw_type,
            proto_type,
            hw_size,
            proto_size,
            opcode,
            sender_mac,
            sender_ip,
            target_mac,
            target_ip,
        })
    }
}
