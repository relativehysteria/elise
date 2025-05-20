//! L1: Ethernet implementation

use crate::net::Mac;
use crate::net::packet::{Packet, PacketCursor, ParseError};

/// A parsed Ethernet header
#[derive(Debug)]
pub struct Parsed<'a> {
    /// Destination device MAC
    pub dst_mac: Mac,

    /// Source device MAC
    pub src_mac: Mac,

    /// Type of the ethernet payload
    pub eth_type: u16,

    /// Raw byte payload
    pub payload: &'a [u8],
}

impl Packet {
    /// Parse the ethernet header
    pub fn parse_eth(&self) -> Result<Parsed, ParseError> {
        let raw = self.raw();

        let dst_mac = Self::parse_mac(raw.get(0x0..0x6))?;
        let src_mac = Self::parse_mac(raw.get(0x6..0xC))?;
        let eth_type = Self::parse_u16(raw.get(0xC..0xE))?;
        let payload = raw.get(0xE..).ok_or(ParseError::TruncatedPacket)?;

        Ok(Parsed { dst_mac, src_mac, eth_type, payload })
    }
}

/// Builder for Ethernet headers
pub struct Builder<'a> {
    pub(super) cursor: PacketCursor<'a>,
}

impl<'a> Builder<'a> {
    /// Creates a new Ethernet builder
    pub fn new(
        mut cursor: PacketCursor<'a>,
        src: &'a Mac,
        dst: &'a Mac
    ) -> Option<Self> {
        // Write the fields
        cursor.write(&dst.0)?;
        cursor.write(&src.0)?;

        Some(Self { cursor })
    }
}
