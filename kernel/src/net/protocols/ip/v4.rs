//! L2: IPv4 implementation

use core::net::Ipv4Addr;

use crate::net::protocols::eth;
use crate::net::packet::{Packet, ParseError, PacketCursor};
use crate::net::protocols::ip::{TransportProtocol, IpBuilder};

/// Ethernet type for IPv4
const ETH_TYPE_IPV4: u16 = 0x0800;

/// A parsed IPv4 header and payload
#[derive(Debug)]
pub struct ParsedV4<'a> {
    /// Ethernet header
    pub eth: eth::Parsed<'a>,

    /// Source IP address
    pub src_ip: Ipv4Addr,

    /// Destination IP address
    pub dst_ip: Ipv4Addr,

    /// IP payload protocol
    pub protocol: u8,

    /// Raw byte payload of the IP packet
    pub payload: &'a [u8],
}

impl Packet {
    /// Parse the IP header
    pub fn parse_ipv4(&self) -> Result<ParsedV4, ParseError> {
        // Parse the Ethernet header
        let eth = self.parse_eth()?;

        // Handle the ethernet type
        if eth.eth_type != ETH_TYPE_IPV4 {
            return Err(ParseError::UnsupportedVersion);
        }

        // Get the header. This will always be at least 20 bytes without options
        let header = eth.payload.get(..20).ok_or(ParseError::InvalidIpHeader)?;

        // Verify the version and the header length
        if (header[0] >> 4)  != 4 {
            return Err(ParseError::UnsupportedVersion);
        }
        if (header[0] & 0xF) != 5 {
            return Err(ParseError::IpOptionsUnsupported);
        }

        // Get the total length of the hader and the payload
        let total_length = Self::parse_u16(Some(&header[2..4]))? as usize;

        // Get the flags and make sure the reserved bit and fragments are clear
        // as we do not support fragmentation yet
        let flags = header[6] >> 5 & 0x7;
        if (flags & 0b101) != 0 {
            return Err(ParseError::FragmentationUnsupported);
        }

        // Make sure there's actually no fragmentation
        let frag_offset = Self::parse_u16(Some(&header[6..8]))?;
        if (frag_offset & 0x1FFF) != 0 {
            return Err(ParseError::FragmentationUnsupported)
        }

        // Get the protocol
        let protocol = header[9];

        // Get the source and destination IPs
        let src_ip = Self::parse_u32(Some(&header[12..16]))?.into();
        let dst_ip = Self::parse_u32(Some(&header[16..20]))?.into();

        // Validate the total length
        if total_length < header.len() || total_length > eth.payload.len() {
            return Err(ParseError::InvalidLength);
        }

        Ok(ParsedV4 {
            src_ip,
            dst_ip,
            protocol,
            payload: &eth.payload[20..total_length],
            eth,
        })
    }
}

impl<'a> eth::Builder<'a> {
    /// Creates a new IPv4 builder from this Ethernet builder
    pub fn ipv4(mut self, src: &'a Ipv4Addr, dst: &'a Ipv4Addr)
            -> Option<BuilderV4<'a>> {
        // Write in the type
        self.cursor.write_u16(ETH_TYPE_IPV4);

        // Split the cursor and save the Ethernet header
        let (_, cursor) = self.cursor.split_at_current();
        BuilderV4::new(cursor, src, dst)
    }
}

/// Indexes to fields which must be filled in when the packet is finalized
struct ToFill {
    len:  usize,
    prot: usize,
    crc:  usize,
}

/// Builder for IPv4 headers
pub struct BuilderV4<'a> {
    hdr:     &'a mut [u8],
    to_fill: ToFill,
    cursor:  Option<PacketCursor<'a>>,
}

impl<'a> BuilderV4<'a> {
    /// Creates a new IPv4 builder
    pub fn new(
        mut cursor: PacketCursor<'a>,
        src: &'a Ipv4Addr,
        dst: &'a Ipv4Addr
    ) -> Option<Self> {
        // Ip version 4 and 20 byte header length
        cursor.write_u8((4 << 4 | 5) as u8)?;

        // No DSCP or ECN
        cursor.write_u8(0)?;

        // Zero out the length for now
        let (len, _) = cursor.write_u16(0)?;

        // Identification, flags and fragment offset are all zero
        cursor.write_u32(0)?;

        // 64 TTL
        cursor.write_u8(64)?;

        // Zero out the protocol. We'll fill this in when the protocol is chosen
        let (prot, _) = cursor.write_u8(0)?;

        // Zero out the checksum. Likewise will be filled in later
        let (crc, _) = cursor.write_u16(0)?;

        // Source and destination IPs
        cursor.write(src.to_bits().to_be_bytes().as_ref())?;
        cursor.write(dst.to_bits().to_be_bytes().as_ref())?;

        // Save off the indexes of the fields which we'll edit later
        let to_fill = ToFill { len, prot, crc };

        // Split off the header
        let (hdr, cursor) = cursor.split_at_current();
        let cursor = Some(cursor);

        Some(Self { hdr, to_fill, cursor, })
    }

    /// Sets the size of this IP header + `len` as the total packet size
    fn write_len(&mut self, len: u16) {
        // Calculate the total size
        let size = ((self.hdr.len() as u16).checked_add(len))
            .expect("totale packet size len overflow");

        // Write it down
        let idx = self.to_fill.len;
        self.hdr[idx..idx + 2].copy_from_slice(&size.to_be_bytes());
    }

    /// Calculates and sets the CRC field of this IP header
    fn write_crc(&mut self) {
        // Calculate the checksum
        let checksum = !Packet::checksum(0, &self.hdr);

        // Write it down
        let idx = self.to_fill.crc;
        self.hdr[idx..idx + 2].copy_from_slice(&checksum.to_ne_bytes());
    }
}

impl<'a> IpBuilder<'a> for BuilderV4<'a> {
    fn set_protocol(&mut self, prot: TransportProtocol) {
        self.hdr[self.to_fill.prot] = prot as u8;
    }

    fn take_cursor(&mut self) -> Option<PacketCursor<'a>> {
        self.cursor.take()
    }

    fn finalize(&mut self, payload_len: u16) {
        self.write_len(payload_len);
        self.write_crc();
    }
}
