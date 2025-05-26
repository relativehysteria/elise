//! Packet interface

use cursor::Cursor;

use crate::net::{Mac, NetDriver};
use crate::mm::ContigPageAligned;

/// Errors that can occur while parsing network packet headers
#[derive(Debug)]
pub enum ParseError {
    /// Indicates the packet is too short to contain the required data for the
    /// given field.
    TruncatedPacket,

    /// The MAC address bytes could not be properly converted into a 6-byte
    /// array
    InvalidMacAddress,

    /// Attempted to parse a big-endian `u16` but got an error
    InvalidWord,

    /// Attempted to parse a big-endian `u32` but got an error
    InvalidDword,

    /// The Ethernet frame indicates a version we do not support
    UnsupportedVersion,

    /// The IP header is either missing or too short to be valid
    InvalidIpHeader,

    /// The IP header included options which we do not support
    IpOptionsUnsupported,

    /// Attempted to parse an IP packet but got invalid protocol
    InvalidIpProtocol,

    /// Fragmentation is not supported and the packet is either fragmented or
    /// has disallowed flags set
    FragmentationUnsupported,

    /// The total packet length is invalid (too short or longer than
    /// available data)
    InvalidLength,
}

/// Allocated packet that can be put into and taken from DMA buffers.
///
/// The inner backing buffer is guaranteed to be page-sized and page-aligned.
pub struct Packet {
    /// The raw backing memory for the packet
    raw: ContigPageAligned<[u8; 4096]>,

    /// Size of the inner backing memory
    length: usize,
}

impl Packet {
    /// Maximum allowed packet length in bytes for a standard Ethernet frame,
    /// excluding FCS.
    const MAX_PACKET_LEN: usize = 1514;

    /// Allocate a new packet buffer
    pub fn new() -> Self {
        Self {
            raw: ContigPageAligned::new([0u8; 4096]),
            length: 0,
        }
    }

    /// Helper function to parse a MAC address from a packet
    pub(super) fn parse_mac(bytes: Option<&[u8]>) -> Result<Mac, ParseError> {
        let slice = bytes.ok_or(ParseError::TruncatedPacket)?;
        slice.try_into().map(Mac).map_err(|_| ParseError::InvalidMacAddress)
    }

    /// Helper function to parse a `u16` from a packet
    pub(super) fn parse_u16(b: Option<&[u8]>) -> Result<u16, ParseError> {
        let slice = b.ok_or(ParseError::TruncatedPacket)?;
        slice.try_into()
            .map(u16::from_be_bytes)
            .map_err(|_| ParseError::InvalidWord)
    }

    /// Helper function to parse a `u32` from an IP packet
    pub(super) fn parse_u32(b: Option<&[u8]>) -> Result<u32, ParseError> {
        let slice = b.ok_or(ParseError::TruncatedPacket)?;
        slice.try_into()
            .map(u32::from_be_bytes)
            .map_err(|_| ParseError::InvalidDword)
    }

    /// Compute a ones-complement checksum over the provided byte slice.
    pub fn checksum(bytes: &[u8]) -> u16 {
        let mut checksum: u32 = 0;

        // Process all 2-byte chunks
        for chunk in bytes.chunks_exact(2) {
            let word = u16::from_be_bytes([chunk[0], chunk[1]]);
            checksum = checksum.wrapping_add(word as u32);
        }

        // Handle final byte (low byte) if length is odd
        if let Some(&last_byte) = bytes.chunks_exact(2).remainder().first() {
            checksum = checksum.wrapping_add((last_byte as u32) << 8);
        }

        // Fold carries
        let checksum = (checksum & 0xFFFF).wrapping_add(checksum >> 16);
        let checksum = (checksum & 0xFFFF).wrapping_add(checksum >> 16);
        checksum as u16
    }

    /// Get the physical address of the packet
    pub fn phys_addr(&self) -> page_table::PhysAddr {
        self.raw.phys_addr()
    }

    /// Get access to the packet contents
    pub fn raw(&self) -> &[u8] {
        &self.raw[..self.length]
    }

    /// Get the length of the packet
    pub fn len(&self) -> usize {
        self.length
    }

    /// Sets the length of the packet
    pub fn set_len(&mut self, len: usize) {
        self.length = len;
    }

    /// Sets the len of the packet to `0`
    pub fn clear(&mut self) {
        self.set_len(0);
    }

    /// Provides a cursor to modify the packet's buffer, ensuring length is
    /// tracked and limited to maximum packet length
    pub fn cursor(&mut self) -> PacketCursor {
        PacketCursor::new(self)
    }
}

/// A cursor that ensures the `Packet`'s length is updated on writes or splits
pub struct PacketCursor<'a> {
    /// Inner cursor over the packet's buffer
    inner: Cursor<'a, u8>,

    /// Reference to the packet's length to update on changes
    packet_len: &'a mut usize,
}

impl<'a> PacketCursor<'a> {
    /// Creates a new `PacketCursor` from the `packet`
    pub fn new(packet: &'a mut Packet) -> Self {
        // Create the cursor
        let cur_pos = packet.len();
        let buffer = packet.raw.as_mut();
        let mut inner = Cursor::new_with_limit(buffer, Packet::MAX_PACKET_LEN);

        // Set the initial position to the current length
        inner.set_position(cur_pos);

        Self {
            inner,
            packet_len: &mut packet.length,
        }
    }

    /// Adjusts the length of the packet to the overall position of the cursor
    fn update_len(&mut self) {
        *self.packet_len = self.inner.overall_position();
    }

    /// Split the current cursor at the current position
    pub fn split_at_current(self) -> (&'a mut [u8], Self) {
        // Split the inner cursor
        let (left, right) = self.inner.split_at_current();

        // Create the new cursor
        let right = Self {
            inner: right,
            packet_len: self.packet_len,
        };

        (left, right)
    }

    /// Non-panic version of `split_at()`
    pub fn split_at_checked(self, idx: usize) -> Option<(&'a mut [u8], Self)> {
        // Split the inner cursor
        let (left, right) = self.inner.split_at_checked(idx)?;

        // Create the new cursor
        let mut right = Self {
            inner: right,
            packet_len: self.packet_len,
        };
        right.update_len();

        Some((left, right))
    }

    /// Splits the cursor into a slice and a new `PacketCursor`, as defined by
    /// the `Cursor::split_at()` semantics
    pub fn split_at(self, idx: usize) -> (&'a mut [u8], Self) {
        self.split_at_checked(idx)
            .expect("Attempted to split PacketCursor with overflow")
    }

    /// Gets a reference to the initialized part of underlying buffer
    pub fn get(&self) -> &[u8] {
        self.inner.get()
    }

    /// Gets a mutable reference to the initialized part of underlying buffer
    pub fn get_mut(&mut self) -> &mut [u8] {
        self.inner.get_mut()
    }

    /// Writes data into the cursor and updates the packet length
    ///
    /// Read the documentation for `Cursor::write()` for more
    pub fn write(&mut self, buf: &[u8]) -> Option<(usize, usize)> {
        // Attempt to write the buffer. This will fail if we go over the limit
        let (old, new) = self.inner.write(buf)?;
        self.update_len();

        Some((old, new))
    }

    /// Writes a `u8` into the cursor using `Cursor::write()`
    pub fn write_u8(&mut self, val: u8) -> Option<(usize, usize)> {
        self.write(val.to_be_bytes().as_ref())
    }

    /// Writes a `u8` into the cursor using `Cursor::write()`
    pub fn write_u16(&mut self, val: u16) -> Option<(usize, usize)> {
        self.write(val.to_be_bytes().as_ref())
    }

    /// Writes a `u8` into the cursor using `Cursor::write()`
    pub fn write_u32(&mut self, val: u32) -> Option<(usize, usize)> {
        self.write(val.to_be_bytes().as_ref())
    }

    /// Writes a `u8` into the cursor using `Cursor::write()`
    pub fn write_u64(&mut self, val: u64) -> Option<(usize, usize)> {
        self.write(val.to_be_bytes().as_ref())
    }
}

/// A lease of a packet
///
/// Whenever a packet is received, the NIC gives a lease to it to the receiving
/// application. When the lease is dropped, the packet is released back to the
/// `owner` of the packet.
///
/// The packet can be extracted from the lease with `take()`, causing it to not
/// get released back to the NIC.
pub struct PacketLease<'a> {
    /// The owner of the packet that has provided this lease
    owner: &'a dyn NetDriver,

    /// The packet that was leased out
    packet: Option<Packet>,
}

impl<'a> PacketLease<'a> {
    /// Create a new packet lease
    pub fn new(owner: &'a dyn NetDriver, packet: Packet) -> Self {
        Self {
            owner,
            packet: Some(packet)
        }
    }

    /// Take ownership of the packet permanently
    pub fn take(mut lease: Self) -> Packet {
        lease.packet.take()
            .expect("Packet already taken out from lease")
    }
}

impl<'a> Drop for PacketLease<'a> {
    fn drop(&mut self) {
        // Release the packet back to the NIC
        if let Some(packet) = self.packet.take() {
            self.owner.release_packet(packet);
        }
    }
}

impl<'a> core::ops::Deref for PacketLease<'a> {
    type Target = Packet;
    fn deref(&self) -> &Self::Target {
        self.packet.as_ref()
            .expect("Packet already taken out of lease")
    }
}

impl<'a> core::ops::DerefMut for PacketLease<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.packet.as_mut()
            .expect("Packet already taken out of lease")
    }
}
