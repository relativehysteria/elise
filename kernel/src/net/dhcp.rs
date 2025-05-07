//! DHCP implementation

use alloc::sync::Arc;

use crate::net::NetDevice;

/// DHCP message header
#[derive(Debug, Clone, Copy, Default)]
#[repr(C, packed)]
struct Header {
    /// Message op code / message type
    op: u8,

    /// Hardware address type
    htype: u8,

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

    /// Optional null-terminated server host name
    sname: [u64; 64 / 8],

    /// Boot file name
    file: [u64; 128 / 8],
}

/// DHCP op code / message type
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
enum Opcode {
    /// Boot request
    Request = 1,

    /// Boot reply
    Reply = 2,
}

/// ARP hardware type
///
/// [Source](https://www.iana.org/assignments/arp-parameters/arp-parameters.xhtml#arp-parameters-2)
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
enum HardwareType {
    /// Ethernet (10Mb)
    Ethernet = 1,
}

/// DHCP client-server message type
///
/// [Source](https://datatracker.ietf.org/doc/html/rfc2131#section-3.1)
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
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
    None
}
