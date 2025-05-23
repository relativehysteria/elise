//! L4: DHCP implementation

#![allow(dead_code)]

use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::net::{Ipv4Addr, IpAddr};

use crate::net::{NetDevice, Port, NetAddress, Mac};
use crate::net::protocols::udp;
use crate::net::packet::Packet;

/// Amount of time in microseconds to wait for a DHCP response.
/// If a response doesn't come within this timeout, the process will be aborted
const DHCP_TIMEOUT: u64 = 5_000_000;

/// DHCP port of the client
const DHCP_CLIENT_PORT: Port = Port(68);

/// DHCP port of the server
const DHCP_SERVER_PORT: Port = Port(67);

/// The magic DHCP cookie
const DHCP_COOKIE: u32 = 0x63825363;

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

    /// The magic DHCP cookie
    cookie: u32,
}

/// Builder for a list of `DhcpOption`s
struct DhcpOptionsBuilder {
    inner: Vec<u8>,
}

impl DhcpOptionsBuilder {
    /// Creates a new options builder
    pub fn new() -> Self {
        Self { inner: Vec::new() }
    }

    /// Adds an option to this builder
    pub fn add(&mut self, option: DhcpOption) -> &mut Self {
        option.serialize(&mut self.inner);
        self
    }

    /// Finishes building the options and returns the serialized slice
    pub fn end(mut self) -> Box<[u8]> {
        DhcpOption::End.serialize(&mut self.inner);
        self.inner.into_boxed_slice()
    }
}

/// DHCP options
#[derive(Clone, Debug, PartialEq, Eq)]
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
#[derive(Debug, PartialEq, Eq)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// A DHCP lease
#[derive(Debug, Clone, Copy)]
pub struct Lease {
    pub client_ip:    Ipv4Addr,
    pub server_ip:    Ipv4Addr,
    pub broadcast_ip: Option<Ipv4Addr>,
    pub subnet_mask:  Option<Ipv4Addr>,
}

/// Attempt to get a DHCP lease for `dev`
pub fn get_lease(dev: Arc<NetDevice>) -> Option<Lease> {
    let xid = cpu::rdtsc() as u32;
    let mac = dev.mac();
    let bind = NetDevice::bind_udp_port(dev.clone(), DHCP_CLIENT_PORT)?;

    let send_dhcp_request = |msg_type: MessageType, extra_opts: &[DhcpOption]| {
        let mut opts = DhcpOptionsBuilder::new();
        opts
            .add(DhcpOption::MessageType(msg_type))
            .add(DhcpOption::ParameterRequestList(&[
                DhcpOptionId::MessageType as u8,
                DhcpOptionId::ServerIp    as u8,
                DhcpOptionId::BroadcastIp as u8,
                DhcpOptionId::SubnetMask  as u8,
            ]));

        for opt in extra_opts.into_iter() {
            opts.add(opt.clone());
        }

        let mut packet = dev.allocate_packet();
        packet.create_dhcp_request(xid, mac, &opts.end());
        dev.send(packet, true);
    };

    // Discover phase
    send_dhcp_request(MessageType::Discover, &[]);

    // Attempt to get the offer IP and server IP
    let mut offered_ip: Option<Ipv4Addr> = None;
    let mut server_ip:  Option<Ipv4Addr> = None;
    bind.recv_timeout(DHCP_TIMEOUT, |_, udp| {
        // Accept packets destined for us
        let dst_mac = udp.ip.eth().dst_mac;
        if dst_mac != mac && dst_mac != Mac::BROADCAST { return None; }

        // Attempt to parse the packet as a DHCP reply
        let (header, options) = udp.parse_dhcp_reply(xid)?;

        // We're looking for an offer now
        options.iter()
            .find(|&x| *x == DhcpOption::MessageType(MessageType::Offer))?;

        // Save the offered IP
        offered_ip = Some(u32::from_be(header.yiaddr).into());

        // Save the server IP as well
        server_ip = options.iter().find_map(|x| {
            if let DhcpOption::ServerIp(ip) = x {
                Some(*ip)
            } else { None }
        });

        Some(())
    })?;

    // Make sure we've received the required IPs
    let offered_ip = offered_ip?;
    let server_ip  = server_ip?;

    // Request phase
    send_dhcp_request(MessageType::Request, &[
        DhcpOption::RequestedIp(offered_ip),
        DhcpOption::ServerIp(server_ip),
    ]);

    // Attempt to get the broadcast IP and the subnet mask
    let mut broadcast_ip: Option<Ipv4Addr> = None;
    let mut subnet_mask:  Option<Ipv4Addr> = None;
    bind.recv_timeout(DHCP_TIMEOUT, |_, udp| {
        // Accept packets destined for us
        let dst_mac = udp.ip.eth().dst_mac;
        if dst_mac != mac && dst_mac != Mac::BROADCAST { return None; }

        // Attempt to parse the packet as a DHCP reply
        let (_header, options) = udp.parse_dhcp_reply(xid)?;

        // We're looking for an ACK now
        options.iter()
            .find(|&x| *x == DhcpOption::MessageType(MessageType::Ack))?;

        // Save the broadcast IP
        broadcast_ip = options.iter().find_map(|x| {
            if let DhcpOption::BroadcastIp(ip) = x {
                Some(*ip)
            } else { None }
        });

        // Save the subnet_mask
        subnet_mask = options.iter().find_map(|x| {
            if let DhcpOption::SubnetMask(ip) = x {
                Some(*ip)
            } else { None }
        });

        Some(())
    })?;

    // Return the lease
    let lease = Lease {
        client_ip: offered_ip,
        server_ip,
        broadcast_ip,
        subnet_mask,
    };
    println!("Got DHCP lease for {mac:X?}! {lease:#X?}");
    Some(lease)
}

impl<'a> udp::Parsed<'a> {
    /// Attempt to parse a DHCPREPLY packet
    fn parse_dhcp_reply(&self, xid: u32)
            -> Option<(Header, Vec<DhcpOption<'a>>)> {

        // Get the header and the options
        let (header_bytes, mut raw_options) = self.payload
            .split_at_checked(core::mem::size_of::<Header>())?;

        // Cast the header bytes as the header
        let header = unsafe { &*(header_bytes.as_ptr() as *const Header) };
        let cookie = u32::from_be(header.cookie);
        let header_xid = u32::from_be(header.xid);

        // Verify transaction ID matches
        if header_xid != xid {
            return None;
        }

        // Check header validity
        if header.op != Opcode::Reply
            || header.htype != HardwareType::Ethernet
            || header.hlen != HardwareType::Ethernet.hlen()
            || cookie != DHCP_COOKIE
        {
            return None;
        }

        // Parse DHCP options
        let mut options = Vec::new();
        while !raw_options.is_empty() {
            if let Some(option) = DhcpOption::parse(&mut raw_options) {
                options.push(option);
            } else {
                break;
            }
        }

        Some((*header, options))
    }
}

impl Packet {
    /// Creates a finalized DHCPREQUEST out of this packet, using `mac` as the
    /// source MAC address
    fn create_dhcp_request(&mut self, xid: u32, mac: Mac, options: &[u8]) {
        // Initialize the address
        let addr = NetAddress {
            src_mac: mac,
            dst_mac: Mac::BROADCAST,
            src_ip: IpAddr::V4(Ipv4Addr::from_bits(0)),
            dst_ip: IpAddr::V4(Ipv4Addr::from_bits(!0)),
            src_port: DHCP_CLIENT_PORT,
            dst_port: DHCP_SERVER_PORT,
        };

        // Create a UDP builder
        let mut builder = self.create_udp(&addr);

        {
            // Write in the empty DHCP header
            builder.write(&[0; core::mem::size_of::<Header>()]);

            // Cast the payload bytes as the header
            let header: &mut Header = unsafe {
                &mut *(builder.payload.get_mut().as_mut_ptr() as *mut Header)
            };

            // Fill the header in
            header.op    = Opcode::Request;
            header.htype = HardwareType::Ethernet;
            header.hlen  = header.htype.hlen();
            header.xid   = xid.to_be();

            // Set our MAC
            header.chaddr[..6].copy_from_slice(&mac.0);

            // Set the DHCP cookie
            header.cookie = DHCP_COOKIE.to_be();
        }

        // Write in the options
        builder.write(options);

        // Here the builder will get dropped and finalized
    }
}
