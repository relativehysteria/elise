//! DHCP implementation

use alloc::sync::Arc;

use crate::net::{NetDevice, Port};

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
#[derive(Debug, Clone, Copy)]
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
    let xid = cpu::rdtsc() as u32;

    // Get the device's MAC
    let mac = dev.mac();

    // Bind to the client DHCP port
    let bind = NetDevice::bind_udp_port(dev.clone(), Port(68))
        .expect("Could not bind to port 68");

    None
}
