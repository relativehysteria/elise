//! L2: IPv6 implementation

use core::net::Ipv6Addr;

use crate::net::protocols::eth;
use crate::net::packet::{Packet, ParseError, PacketCursor};
use crate::net::protocols::ip::{TransportProtocol, IpBuilder};

/// Ethernet type for IPv6
const ETH_TYPE_IPV6: u16 = 0x86DD;

/// A parsed IPv6 header and payload
#[derive(Debug)]
pub struct ParsedV6<'a> {
    /// Ethernet header
    pub eth: eth::Parsed<'a>,

    /// Source IP address
    pub src_ip: Ipv6Addr,

    /// Destination IP address
    pub dst_ip: Ipv6Addr,

    /// Next header protocol
    pub next_header: u8,

    /// Payload
    pub payload: &'a [u8],
}

impl Packet {
    /// Parse the IPv6 header
    pub fn parse_ipv6(&self) -> Result<ParsedV6, ParseError> {
        let eth = self.parse_eth()?;

        // Handle the ethernet type
        if eth.eth_type != ETH_TYPE_IPV6 {
            return Err(ParseError::UnsupportedVersion);
        }

        // Get the header. This will always be at least 40 bytes
        let header = eth.payload.get(..40).ok_or(ParseError::InvalidIpHeader)?;

        // Verify the version and the header length
        if (header[0] >> 4) != 6 {
            return Err(ParseError::UnsupportedVersion);
        }

        // Parse out everything we can get
        let payload_len = Self::parse_u16(Some(&header[4..6]))? as usize;
        let next_header = header[6];
        let src_ip = Ipv6Addr::from(
            <[u8; 16]>::try_from(&header[8..24]).unwrap());
        let dst_ip = Ipv6Addr::from(
            <[u8; 16]>::try_from(&header[24..40]).unwrap());

        // Validate the total length
        if payload_len + header.len() > eth.payload.len() {
            return Err(ParseError::InvalidLength);
        }

        Ok(ParsedV6 {
            src_ip,
            dst_ip,
            next_header,
            payload: &eth.payload[40..40 + payload_len],
            eth,
        })
    }
}

impl<'a> eth::Builder<'a> {
    /// Creates a new IPv6 builder from this Ethernet builder
    pub fn ipv6(mut self, src: &'a Ipv6Addr, dst: &'a Ipv6Addr)
        -> Option<BuilderV6<'a>> {
        // Write in the type
        self.cursor.write_u16(ETH_TYPE_IPV6);

        // Split the cursor and save the ethernet header
        let (_, cursor) = self.cursor.split_at_current();
        BuilderV6::new(cursor, src, dst)
    }
}

/// Builder for IPv6 headers
pub struct BuilderV6<'a> {
    hdr: &'a mut [u8],
    src: Ipv6Addr,
    dst: Ipv6Addr,
    to_fill: ToFillV6,
    cursor: Option<PacketCursor<'a>>,
}

struct ToFillV6 {
    len: usize,
    prot: usize,
}

impl<'a> BuilderV6<'a> {
    pub fn new(
        mut cursor: PacketCursor<'a>,
        src: &'a Ipv6Addr,
        dst: &'a Ipv6Addr,
    ) -> Option<Self> {
        // Version + traffic class
        cursor.write_u8((6 << 4) as u8)?;
        cursor.write_u8(0)?;

        // FLow label
        cursor.write_u16(0)?;

        // Payload length
        let (len, _) = cursor.write_u16(0)?; // payload length

        // Next header/transport protocol
        let (prot, _) = cursor.write_u8(0)?;

        // 64 TTL
        cursor.write_u8(64)?;

        // Source and destination IPs
        cursor.write(src.octets().as_ref())?;
        cursor.write(dst.octets().as_ref())?;

        // Save off the indexes of the fields which we'll edit later
        let to_fill = ToFillV6 { len, prot };

        // Split off the header
        let (hdr, cursor) = cursor.split_at_current();
        let cursor = Some(cursor);

        Some(Self { hdr, to_fill, cursor, src: *src, dst: *dst })
    }

    /// Gets the source IP address this builder was called with
    pub fn src(&self) -> Ipv6Addr {
        self.src
    }

    /// Gets the destination IP address this builder was called with
    pub fn dst(&self) -> Ipv6Addr {
        self.dst
    }

    /// Sets the size of this IP header + `len` as the total packet size
    fn write_len(&mut self, len: u16) {
        let idx = self.to_fill.len;
        self.hdr[idx..idx + 2].copy_from_slice(&len.to_be_bytes());
    }
}

impl<'a> IpBuilder<'a> for BuilderV6<'a> {
    fn set_protocol(&mut self, prot: TransportProtocol) {
        self.hdr[self.to_fill.prot] = prot as u8;
    }

    fn take_cursor(&mut self) -> Option<PacketCursor<'a>> {
        self.cursor.take()
    }

    fn finalize(&mut self, payload_len: u16) {
        self.write_len(payload_len);
    }
}
