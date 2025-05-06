//! The network driver abstraction trait that has to be implemented by all NIC
//! drivers in the kernel

use alloc::boxed::Box;
use alloc::vec::Vec;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicUsize, Ordering};
use core::net::Ipv4Addr;

use oncelock::OnceLock;
use spinlock::SpinLock;

use crate::mm::ContigPageAligned;
use crate::core_locals::InterruptLock;

/// All net devices registered during the PCI probing process. When the
/// probing process ends, these will be locked into `NET_DEVICES`, which
/// can be accessed without locks during runtime.
static PROBED_DEVICES: SpinLock<Option<Vec<Arc<NetDevice>>>, InterruptLock> =
    SpinLock::new(Some(Vec::new()));

/// All networking capable devices on the system
static NET_DEVICES: OnceLock<Box<[Arc<NetDevice>]>> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
#[repr(transparent)]
/// The MAC address of a NIC
pub struct Mac(pub [u8; 6]);

/// A networking capable device
pub struct NetDevice {
    /// A unique ID for this device
    id: usize,

    /// The driver that provides raw RX and TX over the network
    driver: Arc<dyn NetDriver>,

    /// The MAC address of this device
    mac: Mac,
}

impl NetDevice {
    /// Get the likely least contended `NetDevice` on the system
    ///
    /// "Likely" here means you will get one of the least contended devices,
    /// potentially not the least contended one.
    ///
    /// This is an inherent drawback of the lockless nature of this function.
    pub fn get() -> Option<Arc<Self>> {
        // The least contended dev on the system
        let mut ret: Option<Arc<Self>> = None;

        // Do no return any device before the PCI probing process ends
        if !NET_DEVICES.initialized() { return ret; }

        // Go through all devices, looking for the least contended one
        for dev in NET_DEVICES.get().iter() {
            // Compute the current best strong count for a net device
            let cur_best = ret.as_ref()
                // -1 because `ret` increases the count by 1
                .map(|x| Arc::strong_count(x) - 1)
                .unwrap_or(!0);

            // If this device has fewer references, use this device
            if Arc::strong_count(&dev) < cur_best {
                ret = Some(dev.clone());
            }
        }

        ret
    }

    /// Register a device during the PCI probing process as a network device
    pub fn register(driver: Arc<dyn NetDriver>) {
        /// The next available unique identifier
        static NEXT_DEV_ID: AtomicUsize = AtomicUsize::new(0);

        // Don't allow hotplugging net devices
        if NET_DEVICES.initialized() {
            panic!("Net devices have already been locked in!");
        }

        // Get a new unique ID
        let id = NEXT_DEV_ID.fetch_add(1, Ordering::SeqCst);
        id.checked_add(1).expect("Net device unique ID overflow");

        // Create a new `Arc<NetDevice>`
        let nd = Arc::new(Self {
            mac: driver.mac(),
            driver,
            id,
        });

        // Register it
        let mut devs = PROBED_DEVICES.lock();
        let devs = devs.as_mut().unwrap();
        devs.push(nd);
    }

    /// Lock in all of the registered net devices on the system, marking them
    /// for use
    pub fn lock_in() {
        let devs = PROBED_DEVICES.lock().take()
            .expect("Net devices locked in already!");
        NET_DEVICES.set(devs.into_boxed_slice());
    }

    /// Get the device's unique identifier
    pub fn id(&self) -> usize {
        self.id
    }
}

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

/// Errors that can occur while parsing network packet headers
#[derive(Debug)]
pub enum ParseError {
    /// Indicates the packet is too short to contain the required data for the
    /// given field.
    TruncatedPacket,

    /// The MAC address bytes could not be properly converted into a 6-byte
    /// array
    InvalidMacAddress,

    /// The Ethernet type field could not be parsed due to incorrect byte size
    /// or layout
    InvalidEtherType,

    /// The Ethernet frame indicates a version we do not support
    UnsupportedVersion,

    /// The IP header is either missing or too short to be valid
    InvalidIpHeader,

    /// The IP header included options which we do not support
    IpOptionsUnsupported,

    /// Fragmentation is not supported and the packet is either fragmented or
    /// has disallowed flags set
    FragmentationUnsupported,

    /// The total IP packet length is invalid (too short or longer than
    /// available data)
    InvalidLength,
}

#[derive(Debug)]
/// A parsed Ethernet header and payload
pub struct Ethernet<'a> {
    /// Destination MAC address
    pub dst_mac: Mac,

    /// Source MAC address
    pub src_mac: Mac,

    /// Type of the Ethernet payload
    pub eth_type: u16,

    /// Raw bytes following the header
    pub payload: &'a [u8],
}

/// A parsed IP header and payload
pub struct Ip<'a> {
    /// Ethernet header
    pub eth: Ethernet<'a>,

    /// Source IP address
    pub src_ip: Ipv4Addr,

    /// Destination IP address
    pub dst_ip: Ipv4Addr,

    /// IP payload protocol
    pub protocol: u8,

    /// Raw byte payload of the IP packet
    pub payload: &'a [u8],
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

    /// Parse the ethernet header
    pub fn eth(&self) -> Result<Ethernet, ParseError> {
        let raw = self.raw();

        let dst_mac = Self::parse_mac(raw.get(0x0..0x6))?;
        let src_mac = Self::parse_mac(raw.get(0x6..0xC))?;
        let eth_type = Self::parse_u16(
            raw.get(0xC..0xE), ParseError::InvalidEtherType)?;
        let payload = raw.get(0xE..).ok_or(ParseError::TruncatedPacket)?;

        Ok(Ethernet { dst_mac, src_mac, eth_type, payload })
    }

    /// Parse the IP header
    pub fn ip(&self) -> Result<Ip, ParseError> {
        // Parse the Ethernet header
        let eth = self.eth()?;

        // We handle IPv4 only for now
        const ETH_TYPE_IPV4: u16 =  0x0800;
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
        let total_length = Self::parse_u16(
            Some(&header[2..4]), ParseError::InvalidLength)? as usize;

        // Get the flags and make sure the reserved bit and fragments are clear
        // as we do not support fragmentation yet
        let flags = header[6] >> 5 & 0x7;
        if (flags & 0b101) != 0 {
            return Err(ParseError::FragmentationUnsupported);
        }

        // Make sure there's actually no fragmentation
        let frag_offset = Self::parse_u16(
            Some(&header[6..8]), ParseError::InvalidIpHeader)?;
        if (frag_offset & 0x1FFF) != 0 {
            return Err(ParseError::FragmentationUnsupported)
        }

        // Get the protocol
        let protocol = header[9];

        // Get the source and destination IPs
        let src_ip = Self::parse_u32(Some(&header[12..16]))?.into();
        let dst_ip = Self::parse_u32(Some(&header[16..20]))?.into();

        // Validate the total length
        if total_length > eth.payload.len() {
            return Err(ParseError::InvalidLength);
        }

        Ok(Ip {
            src_ip,
            dst_ip,
            protocol,
            payload: &eth.payload[20..total_length],
            eth,
        })
    }

    /// Helper function to parse a MAC address from a packet
    fn parse_mac(bytes: Option<&[u8]>) -> Result<Mac, ParseError> {
        let slice = bytes.ok_or(ParseError::TruncatedPacket)?;
        slice.try_into().map(Mac).map_err(|_| ParseError::InvalidMacAddress)
    }

    /// Helper function to parse a `u16` from a packet
    fn parse_u16(b: Option<&[u8]>, err: ParseError) -> Result<u16, ParseError> {
        let slice = b.ok_or(ParseError::TruncatedPacket)?;
        slice.try_into()
            .map(u16::from_be_bytes)
            .map_err(|_| err)
    }

    /// Helper function to parse a `u32` from an IP packet
    fn parse_u32(b: Option<&[u8]>) -> Result<u32, ParseError> {
        let slice = b.ok_or(ParseError::InvalidIpHeader)?;
        slice.try_into()
            .map(u32::from_be_bytes)
            .map_err(|_| ParseError::InvalidIpHeader)
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
            .expect("Packet already taken out of lease")
    }
}

impl<'a> core::ops::DerefMut for PacketLease<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.packet.as_mut()
            .expect("Packet already taken out of lease")
    }
}
