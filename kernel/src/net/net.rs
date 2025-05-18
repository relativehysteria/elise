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

use crate::core_locals::InterruptLock;
use crate::net::protocols::dhcp;
use crate::net::packet::{Packet, PacketLease};

/// All net devices registered during the PCI probing process. When the
/// probing process ends, these will be locked into `NET_DEVICES`, which
/// can be accessed without locks during runtime.
static PROBED_DEVICES: SpinLock<Option<Vec<Arc<NetDevice>>>, InterruptLock> =
    SpinLock::new(Some(Vec::new()));

/// All networking capable devices on the system
static NET_DEVICES: OnceLock<Box<[Arc<NetDevice>]>> = OnceLock::new();

/// Type signifying the 'rest' part of a packet buffer split by any of the
/// `split_at_mut()` methods
pub type Payload<'a> = &'a mut [u8];

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

    // TODO:
    // pub tcp_connections:
    //    SpinLock<BTreeMap<Port, Arc<SpinLock<TcpConnection, InterruptLock>>>>,
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

        // // If we can't get a DHCP lease for some device, we won't use it
        // let mut leased_devs = Vec::with_capacity(devs.len());

        // // Attempt to get a DHCP lease for all devices
        // for dev in devs {
        //     // Get the lease
        //     let lease = dhcp::get_lease(dev.clone());

        //     // Assign the lease
        //     let mut dev_lease = dev.dhcp_lease.lock();
        //     *dev_lease = lease;

        //     // If we actually got a lease, save this device
        //     if dev_lease.is_some() {
        //         leased_devs.push(dev.clone());
        //     }
        // }

        // Save the devices that got a DHCP lease
        NET_DEVICES.set(devs.into_boxed_slice());
    }

    pub fn discard(&self, packet: PacketLease) {
        self.discard_udp(packet);
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

    pub fn driver(&self) -> Arc<dyn NetDriver> {
        self.driver.clone()
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
