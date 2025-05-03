//! The network driver abstraction trait that has to be implemented by all NIC
//! drivers in the kernel

use crate::mm::ContigPageAligned;

#[derive(Debug, Clone, PartialEq, Eq)]
#[repr(transparent)]
/// The MAC address of a NIC
pub struct Mac(pub [u8; 6]);

/// The driver trait that allows access to NIC RX and TX
pub trait NetDriver: Send + Sync {
    /// Forcibly reset the NIC
    unsafe fn reset(&self);

    /// Get the MAC address of the NIC
    fn mac(&self) -> Mac;

    /// Send a raw frame over the network. This `packet` does not include the
    /// FCS; the driver must compute and insert it.
    fn send(&self, packet: Packet, flush: bool);

    /// Receive a raw frame from the network.
    ///
    /// The received packet length must not include the FCS and the FCS must be
    /// validated by the driver
    ///
    /// This `PacketLease` takes ownership of the packet from the NIC for the
    /// duration the packet is needed, and is released back to the NIC when the
    /// lease is dropped.
    fn recv<'a: 'b, 'b>(&'a self) -> Option<PacketLease<'b>>;

    /// Get a packet whose ownership can be given to the NIC during a `send()`
    /// call.
    ///
    /// It is advised the NIC implements its own packet free list to avoid
    /// frequent allocations.
    fn allocate_packet(&self) -> Packet {
        // Allocate a new packet by default
        Packet::new()
    }

    /// Give the packet back to the NIC that gave it to us.
    fn release_packet(&self, _packet: Packet) {
        // Drop/free the packet by default
    }
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
    /// Allocate a new packet buffer
    pub fn new() -> Self {
        Self {
            raw: ContigPageAligned::new([0u8; 4096]),
            length: 0,
        }
    }

    /// Get the physical address of the packet
    pub fn phys_addr(&self) -> page_table::PhysAddr {
        self.raw.phys_addr()
    }

    /// Get access to the packet contents
    pub fn raw(&self) -> &[u8] {
        &self.raw[..self.length]
    }

    /// Get mutable access to the packet contents
    pub fn raw_mut(&mut self) -> &mut [u8] {
        &mut self.raw[..self.length]
    }

    /// Get the length of the packet
    pub fn len(&self) -> usize {
        self.length
    }

    /// Set length of the packet
    pub fn set_len(&mut self, len: usize) {
        assert!(len <= 1514 && len <= self.raw.len(),
            "Attempted to set length out of bounds of the packet data");
        self.length = len;
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
            .expect("Packet taken out from lease")
    }
}

impl<'a> core::ops::DerefMut for PacketLease<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.packet.as_mut()
            .expect("Packet taken out from lease")
    }
}
