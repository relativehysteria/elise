//! Interfaces for the seamless usage of ipv4/ipv6 parsers and builders

use core::net::IpAddr;

use crate::net::protocols::ip::*;
use crate::net::protocols::eth;
use crate::net::packet::PacketCursor;

/// IP transport protocol
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
#[repr(u8)]
pub enum TransportProtocol {
    Icmp = 0x01,
    Tcp  = 0x06,
    Udp  = 0x11,
}

/// Unified representation of parsed IP headers
#[derive(Debug)]
pub enum Parsed<'a> {
    V4(ParsedV4<'a>),
    V6(ParsedV6<'a>),
}

impl<'a> Parsed<'a> {
    /// Get the parsed ethernet header
    pub fn eth(&self) -> &eth::Parsed<'a> {
        match self {
            Parsed::V4(p) => &p.eth,
            Parsed::V6(p) => &p.eth,
        }
    }

    /// Get the source IP address
    pub fn src_ip(&self) -> IpAddr {
        match self {
            Parsed::V4(p) => IpAddr::V4(p.src_ip),
            Parsed::V6(p) => IpAddr::V6(p.src_ip),
        }
    }

    /// Get the destination IP address
    pub fn dst_ip(&self) -> IpAddr {
        match self {
            Parsed::V4(p) => IpAddr::V4(p.dst_ip),
            Parsed::V6(p) => IpAddr::V6(p.dst_ip),
        }
    }

    /// Get the transport protocol
    pub fn protocol(&self) -> u8 {
        match self {
            Parsed::V4(p) => p.protocol,
            Parsed::V6(p) => p.next_header,
        }
    }

    /// Get the IP payload
    pub fn payload(&self) -> &'a [u8] {
        match self {
            Parsed::V4(p) => p.payload,
            Parsed::V6(p) => p.payload,
        }
    }
}

/// Unified representation of the IP builders
pub enum Builder<'a> {
    V4(BuilderV4<'a>),
    V6(BuilderV6<'a>),
}

impl<'a> Builder<'a> {
    /// Set the transport protocol of the payload
    pub fn set_protocol(&mut self, protocol: TransportProtocol) {
        match self {
            Builder::V4(b) => b.set_protocol(protocol),
            Builder::V6(b) => b.set_protocol(protocol),
        }
    }

    /// Take out the cursor out of the builder
    pub fn take_cursor(&mut self) -> Option<PacketCursor<'a>> {
        match self {
            Builder::V4(b) => b.take_cursor(),
            Builder::V6(b) => b.take_cursor(),
        }
    }

    /// Finalize the IP header, writing in the `payload_len` and calculating the
    /// crc (if applicable). This `payload_len` does not include the IP header
    /// size, only the tranport layer size
    pub fn finalize(&mut self, payload_len: u16) {
        match self {
            Builder::V4(b) => b.finalize(payload_len),
            Builder::V6(b) => b.finalize(payload_len),
        }
    }
}

impl<'a> eth::Builder<'a> {
    /// Creates a new IPv4 builder from this Ethernet builder
    pub fn ip(self, src: &'a IpAddr, dst: &'a IpAddr)
            -> Option<Builder<'a>> {
        match (src, dst) {
            (IpAddr::V4(src), IpAddr::V4(dst)) =>
                self.ipv4(src, dst).map(Builder::V4),
            (IpAddr::V6(src), IpAddr::V6(dst)) =>
                self.ipv6(src, dst).map(Builder::V6),
            _ => None,
        }
    }
}
