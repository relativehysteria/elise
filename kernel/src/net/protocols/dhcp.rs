//! DHCP implementation

#![allow(dead_code)]

use alloc::sync::Arc;
use alloc::vec::Vec;
use core::net::Ipv4Addr;

use crate::net::{NetDevice, Port};

/// DHCP port of the client
const DHCP_CLIENT_PORT: Port = Port(68);

/// DHCP port of the server
const DHCP_SERVER_PORT: Port = Port(67);

/// DHCP message header
#[derive(Debug, Clone, Copy, Default)]
#[repr(C, packed)]
struct Header {
    /// Message opcode
    op: Opcode,

    /// Hardware address type
    htype: HardwareType,

    /// Hardware address length
    hlen: u8,

    /// Optionally used by relay agents when booting via a relay agent
    ///
    /// Client sets to 0
    hops: u8,

    /// Transaction ID
    xid: u32,

    /// Seconds elapsed since client began address acquisition or renewal
    /// process
    secs: u16,

    /// DHCP flags
    flags: u16,

    /// Client IP address
    ciaddr: u32,

    /// "Your" (client) IP address
    yiaddr: u32,

    /// IP address of the next server to use in bootstrap
    siaddr: u32,

    /// Relay agent IP address
    giaddr: u32,

    /// Client hardware address
    chaddr: [u8; 16],

    // The u64 here is just a hack to allow it to derive Default
    /// Optional null-terminated server host name
    sname: [u64; 64 / 8],

    // The u64 here is just a hack to allow it to derive Default
    /// Boot file name
    file: [u64; 128 / 8],
}

/// DHCP options
#[derive(Debug, PartialEq, Eq)]
#[allow(missing_docs)]
enum DhcpOption<'a> {
    Pad,
    SubnetMask(Ipv4Addr),
    BroadcastIp(Ipv4Addr),
    RequestedIp(Ipv4Addr),
    LeaseTime(u32),
    MessageType(MessageType),
    ServerIp(Ipv4Addr),
    ParameterRequestList(&'a [u8]),
    RenewalTime(u32),
    Unknown(u8, &'a [u8]),
    End,
}

/// Mapping of DHCP options to their IDs
#[repr(u8)]
#[allow(missing_docs)]
enum DhcpOptionId {
    Pad                  = 0,
    SubnetMask           = 1,
    BroadcastIp          = 28,
    RequestedIp          = 50,
    LeaseTime            = 51,
    MessageType          = 53,
    ServerIp             = 54,
    ParameterRequestList = 55,
    RenewalTime          = 58,
    End                  = 255,
}

impl TryFrom<u8> for DhcpOptionId {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0   => Ok(Self::Pad),
            1   => Ok(Self::SubnetMask),
            28  => Ok(Self::BroadcastIp),
            50  => Ok(Self::RequestedIp),
            51  => Ok(Self::LeaseTime),
            53  => Ok(Self::MessageType),
            54  => Ok(Self::ServerIp),
            55  => Ok(Self::ParameterRequestList),
            58  => Ok(Self::RenewalTime),
            255 => Ok(Self::End),
            _   => Err(()),
        }
    }
}

impl<'a> DhcpOption<'a> {
    /// Parse a DHCP option from a raw message, updating the message pointer to
    /// reflect the number of parsed bytes
    fn parse(ptr: &mut &'a [u8]) -> Option<Self> {
        let code = *ptr.first()?;

        // Handle single-byte options first
        if let Ok(DhcpOptionId::Pad) = DhcpOptionId::try_from(code) {
            *ptr = &ptr[1..];
            return Some(Self::Pad);
        }

        if let Ok(DhcpOptionId::End) = DhcpOptionId::try_from(code) {
            *ptr = &ptr[1..];
            return Some(Self::End);
        }

        // Handle variable-length options
        let len = *ptr.get(1)? as usize;
        let payload = ptr.get(2..2 + len)?;

        let option = match DhcpOptionId::try_from(code) {
            Ok(id) => match id {
                DhcpOptionId::SubnetMask => {
                    let bytes: [u8; 4] = payload.try_into().ok()?;
                    Self::SubnetMask(Ipv4Addr::from(u32::from_be_bytes(bytes)))
                }
                DhcpOptionId::BroadcastIp => {
                    let bytes: [u8; 4] = payload.try_into().ok()?;
                    Self::BroadcastIp(Ipv4Addr::from(u32::from_be_bytes(bytes)))
                }
                DhcpOptionId::RequestedIp => {
                    let bytes: [u8; 4] = payload.try_into().ok()?;
                    Self::RequestedIp(Ipv4Addr::from(u32::from_be_bytes(bytes)))
                }
                DhcpOptionId::LeaseTime => {
                    let bytes: [u8; 4] = payload.try_into().ok()?;
                    Self::LeaseTime(u32::from_be_bytes(bytes))
                }
                DhcpOptionId::MessageType => {
                    let byte: u8 = payload.get(0).copied()?;
                    Self::MessageType(MessageType::try_from(byte).ok()?)
                }
                DhcpOptionId::ServerIp => {
                    let bytes: [u8; 4] = payload.try_into().ok()?;
                    Self::ServerIp(Ipv4Addr::from(u32::from_be_bytes(bytes)))
                }
                DhcpOptionId::ParameterRequestList => {
                    Self::ParameterRequestList(payload)
                }
                DhcpOptionId::RenewalTime => {
                    let bytes: [u8; 4] = payload.try_into().ok()?;
                    Self::RenewalTime(u32::from_be_bytes(bytes))
                }
                _ => unreachable!(), // Handled single-byte variants already
            },
            Err(_) => Self::Unknown(code, payload),
        };

        *ptr = &ptr[2 + len..];
        Some(option)
    }

    /// Serialize a DHCP option by appending it to `buffer`
    fn serialize(&self, buffer: &mut Vec<u8>) {
        match self {
            Self::Pad => buffer.push(DhcpOptionId::Pad as u8),
            Self::End => buffer.push(DhcpOptionId::End as u8),
            Self::SubnetMask(addr) =>
                Self::push_ip_option(buffer, DhcpOptionId::SubnetMask, addr),
            Self::BroadcastIp(addr) =>
                Self::push_ip_option(buffer, DhcpOptionId::BroadcastIp, addr),
            Self::RequestedIp(addr) =>
                Self::push_ip_option(buffer, DhcpOptionId::RequestedIp, addr),
            Self::ServerIp(addr) =>
                Self::push_ip_option(buffer, DhcpOptionId::ServerIp, addr),
            Self::LeaseTime(time) =>
                Self::push_u32_option(buffer, DhcpOptionId::LeaseTime, *time),
            Self::RenewalTime(time) =>
                Self::push_u32_option(buffer, DhcpOptionId::RenewalTime, *time),
            Self::MessageType(typ) => {
                buffer.push(DhcpOptionId::MessageType as u8);
                buffer.push(1);
                buffer.push(*typ as u8);
            }
            Self::ParameterRequestList(data) => {
                buffer.push(DhcpOptionId::ParameterRequestList as u8);
                buffer.push(data.len() as u8);
                buffer.extend_from_slice(data);
            }
            Self::Unknown(code, data) => {
                buffer.push(*code);
                buffer.push(data.len() as u8);
                buffer.extend_from_slice(data);
            }
        }
    }

    // Helper functions for common patterns

    fn push_ip_option(buffer: &mut Vec<u8>, code: DhcpOptionId, ip: &Ipv4Addr) {
        buffer.push(code as u8);
        buffer.push(4);
        buffer.extend_from_slice(&ip.octets());
    }

    fn push_u32_option(buffer: &mut Vec<u8>, code: DhcpOptionId, value: u32) {
        buffer.push(code as u8);
        buffer.push(4);
        buffer.extend_from_slice(&value.to_be_bytes());
    }
}

/// DHCP op code / message type
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
#[allow(missing_docs)]
enum Opcode {
    Request = 1,
    Reply = 2,
}

impl Default for Opcode {
    fn default() -> Self {
        Self::Request
    }
}

/// ARP hardware type
///
/// [Source](https://www.iana.org/assignments/arp-parameters/arp-parameters.xhtml#arp-parameters-2)
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
#[allow(missing_docs)]
enum HardwareType {
    Ethernet = 1,
}

impl HardwareType {
    /// Get the hardware address length based on this type
    fn hlen(&self) -> u8 {
        match self {
            Self::Ethernet => 6,
        }
    }
}

impl Default for HardwareType {
    fn default() -> Self {
        Self::Ethernet
    }
}

/// DHCP client-server message type
///
/// [Source](https://datatracker.ietf.org/doc/html/rfc2131#section-3.1)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
#[allow(missing_docs)]
enum MessageType {
    Discover = 1,
    Offer    = 2,
    Request  = 3,
    Ack      = 5,
    Unsupported,
}

impl From<u8> for MessageType {
    fn from(val: u8) -> Self {
        match val {
            1 => Self::Discover,
            2 => Self::Offer,
            3 => Self::Request,
            5 => Self::Ack,
            _ => Self::Unsupported,
        }
    }
}

pub struct Lease;

/// Attempt to get a DHCP lease for `dev`
pub fn get_lease(dev: Arc<NetDevice>) -> Option<Lease> {
    // Get a unique transaction ID
    let _xid = cpu::rdtsc() as u32;

    // Get the device's MAC
    let _mac = dev.mac();

    // Bind to the client DHCP port
    let _bind = NetDevice::bind_udp_port(dev.clone(), DHCP_CLIENT_PORT)
        .expect("Could not bind to port 68");

    // Construct the DHCP options for the discover
    let mut options = Vec::new();
    DhcpOption::MessageType(MessageType::Discover).serialize(&mut options);
    DhcpOption::ParameterRequestList(&[
        DhcpOptionId::MessageType as u8,
        DhcpOptionId::ServerIp as u8,
    ]).serialize(&mut options);
    DhcpOption::End.serialize(&mut options);

    // Send the packet
    let _packet = dev.allocate_packet();

    None
}

// /// Create a DHCP packet
// fn create_dhcp_packet(packet: &mut Packet, xid: u32, mac: Mac, options: &[u8]) {
//     // Create the broadcast address info
//     let addr = NetAddress {
//         src_mac:  mac,
//         dst_mac:  Mac([0xFF; 6]),
//         src_ip:   Ipv4Addr::from_bits(0),
//         dst_ip:   Ipv4Addr::from_bits(!0),
//         src_port: DHCP_CLIENT_PORT,
//         dst_port: DHCP_SERVER_PORT,
//     };
//
//     // Create the UDP packet
//     let mut pkt = packet.create_udp(&addr);
// }
