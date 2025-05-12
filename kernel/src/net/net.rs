//! The network driver abstraction trait that has to be implemented by all NIC
//! drivers in the kernel

use alloc::boxed::Box;
use alloc::vec::Vec;
use alloc::sync::Arc;
use alloc::collections::{BTreeMap, VecDeque};
use core::sync::atomic::{AtomicU16, AtomicUsize, Ordering};
use core::net::Ipv4Addr;

use oncelock::OnceLock;
use spinlock::SpinLock;

use crate::mm::ContigPageAligned;
use crate::core_locals::InterruptLock;
use crate::net::dhcp;

/// All net devices registered during the PCI probing process. When the
/// probing process ends, these will be locked into `NET_DEVICES`, which
/// can be accessed without locks during runtime.
static PROBED_DEVICES: SpinLock<Option<Vec<Arc<NetDevice>>>, InterruptLock> =
    SpinLock::new(Some(Vec::new()));

/// All networking capable devices on the system
static NET_DEVICES: OnceLock<Box<[Arc<NetDevice>]>> = OnceLock::new();

/// The MAC address of a NIC
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Mac(pub [u8; 6]);

/// A network port
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Port(pub u16);

impl Port {
    /// Allocates a new port within the IANA ephemeral/dynamic range
    /// (`49152..u16::MAX)`.
    ///
    /// Note: This is a logical allocation - it does not check actual port
    /// availability. That is, This port may be already bound if explicitly
    /// bound by the user, at which point this function should be called again
    /// to receive the next possibly still unbound port.
    pub fn next_free() -> Self {
        /// This is the first port that has been specified by IANA as
        /// ephemeral/dynamic.
        const EPHEMERAL_START: u16 = 49152;

        /// The next free port that can be allocated by a function, guaranteed
        /// to be ephemeral as specified by IANA.
        static NEXT_FREE_PORT: AtomicU16 = AtomicU16::new(EPHEMERAL_START);

        // Get the next free port, wrapping around to ephemeral start
        let port = NEXT_FREE_PORT.fetch_update(
            Ordering::SeqCst,
            Ordering::SeqCst,
            |prev| {
                if prev == u16::MAX {
                    Some(EPHEMERAL_START)
                } else {
                    Some(prev + 1)
                }
            })
        .unwrap();

        Self(port)
    }
}

/// UDP/TCP address
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[allow(missing_docs)]
pub struct NetAddress {
    pub src_mac:  Mac,
    pub src_ip:   Ipv4Addr,
    pub src_port: Port,

    pub dst_mac:  Mac,
    pub dst_ip:   Ipv4Addr,
    pub dst_port: Port,
}

impl Default for NetAddress {
    fn default() -> Self {
        Self {
            src_mac:  Default::default(),
            dst_mac:  Default::default(),
            src_port: Default::default(),
            dst_port: Default::default(),
            src_ip:   Ipv4Addr::from_bits(u32::default()),
            dst_ip:   Ipv4Addr::from_bits(u32::default()),
        }
    }
}

/// A networking capable device
pub struct NetDevice {
    /// A unique ID for this device
    id: usize,

    /// The driver that provides raw RX and TX over the network
    driver: Arc<dyn NetDriver>,

    /// The MAC address of this device
    mac: Mac,

    /// The DHCP lease for this device
    pub dhcp_lease: SpinLock<Option<dhcp::Lease>, InterruptLock>,

    /// Packet queues for bound UDP ports
    pub udp_binds: SpinLock<BTreeMap<Port, VecDeque<Packet>>, InterruptLock>,
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
            dhcp_lease: SpinLock::new(None),
            mac: driver.mac(),
            udp_binds: SpinLock::new(BTreeMap::new()),
            driver,
            id,
        });

        // Register it
        PROBED_DEVICES.lock().as_mut().unwrap().push(nd);
    }

    /// Lock in all of the registered net devices on the system, marking them
    /// for use
    pub fn lock_in() {
        // Take the netdevices from the register
        let devs = PROBED_DEVICES.lock().take()
            .expect("Net devices locked in already!");

        // If we can't get a DHCP lease for some device, we won't use it
        let mut leased_devs = Vec::with_capacity(devs.len());

        // Attempt to get a DHCP lease for all devices
        for dev in devs {
            // Get the lease
            let lease = dhcp::get_lease(dev.clone());

            // Assign the lease
            let mut dev_lease = dev.dhcp_lease.lock();
            *dev_lease = lease;

            // If we actually got a lease, save this device
            if dev_lease.is_some() {
                leased_devs.push(dev.clone());
            }
        }

        // Save the devices that got a DHCP lease
        NET_DEVICES.set(leased_devs.into_boxed_slice());
    }

    /// Get the device's unique identifier
    pub fn id(&self) -> usize {
        self.id
    }

    /// Receive a raw packet from the network
    pub fn recv(&self) -> Option<PacketLease> {
        self.driver.recv()
    }

    /// Send a raw packet over the network
    ///
    /// The `packet` must not include the FCS as that will be computed by the
    /// driver.
    pub fn send(&self, packet: Packet, flush: bool) {
        self.driver.send(packet, flush);
    }

    /// Allocate a new packet for use
    pub fn allocate_packet(&self) -> Packet {
        self.driver.allocate_packet()
    }

    /// Get this device's MAC address
    pub fn mac(&self) -> Mac {
        self.mac
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

    /// The total IP packet length is invalid (too short or longer than
    /// available data)
    InvalidLength,
}

/// A parsed Ethernet header and payload
#[derive(Debug)]
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
#[derive(Debug)]
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
        let eth_type = Self::parse_u16(raw.get(0xC..0xE))?;
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
        if total_length < 20 || total_length > eth.payload.len() {
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
    pub fn parse_u16(b: Option<&[u8]>) -> Result<u16, ParseError> {
        let slice = b.ok_or(ParseError::TruncatedPacket)?;
        slice.try_into()
            .map(u16::from_be_bytes)
            .map_err(|_| ParseError::InvalidWord)
    }

    /// Helper function to parse a `u32` from an IP packet
    pub fn parse_u32(b: Option<&[u8]>) -> Result<u32, ParseError> {
        let slice = b.ok_or(ParseError::TruncatedPacket)?;
        slice.try_into()
            .map(u32::from_be_bytes)
            .map_err(|_| ParseError::InvalidDword)
    }

    /// Compute a ones-complement checksum over the provided byte slice.
    fn checksum(bytes: &[u8]) -> u16 {
        let mut sum: u32 = 0;

        // Process all 2-byte chunks
        for chunk in bytes.chunks_exact(2) {
            let word = u16::from_ne_bytes([chunk[0], chunk[1]]);
            sum = sum.wrapping_add(word as u32);
        }

        // Handle final byte (low byte) if length is odd
        if let Some(&last_byte) = bytes.chunks_exact(2).remainder().first() {
            sum = sum.wrapping_add(last_byte as u32);
        }

        // Fold carries
        let sum = (sum & 0xFFFF).wrapping_add(sum >> 16);
        let sum = (sum & 0xFFFF).wrapping_add(sum >> 16);
        sum as u16
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
