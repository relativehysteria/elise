//! Intel e1000 driver

use alloc::sync::Arc;
use alloc::vec::Vec;
use core::mem::size_of;

use spinlock::SpinLock;
use const_assert::const_assert;
use page_table::{
    PhysAddr, VirtAddr, PageType, PAGE_WRITE, PAGE_NXE, PAGE_PRESENT,
    PAGE_CACHE_DISABLE};

use crate::pci::{DeviceConfig, Device, BarBits, BarType};
use crate::mm;
use crate::net::{NetDriver, NetDevice, Mac};
use crate::net::packet::{Packet, PacketLease};
use crate::core_locals::InterruptLock;

/// The Intel NICs map in 128KiB of memory
const MMIO_SIZE: usize = 128 * 1024;

// Make sure the descriptor tables fit on a single page. Each descriptor is
// 16-bytes in size, so there should be at most 4096 / 16 = 256 descs.
// They must also be 128-byte aligned, so that's 128 / 16 = 8 descs.

/// Number of receive descriptors to allocate for a NIC
const RX_DESCS_N: usize = 256;
const_assert!(RX_DESCS_N <= 256);
const_assert!(RX_DESCS_N % 8 == 0);

/// Number of transmit descriptors to allocate for a NIC
const TX_DESCS_N: usize = 256;
const_assert!(TX_DESCS_N <= 256);
const_assert!(TX_DESCS_N % 8 == 0);

/// NIC register offsets
#[derive(Clone, Copy)]
struct NicRegisters {
    /// Device control
    ctrl: usize,

    /// Interrupt mask clear
    imc: usize,

    /// Receive Control
    rctl: usize,

    /// Receive address low
    ral: usize,

    /// Receive address high
    rah: usize,

    /// Receive descriptor base low
    rdbal: usize,

    /// Receive descriptor base high
    rdbah: usize,

    /// Receive descriptor length
    rdlen: usize,

    /// Receive descriptor head
    rdh: usize,

    /// Receive descriptor tail
    rdt: usize,

    /// Transmit control
    tctl: usize,

    /// Transmit descriptor base low
    tdbal: usize,

    /// Transmit descriptor base high
    tdbah: usize,

    /// Transmit descriptor length
    tdlen: usize,

    /// Transmit descriptor head
    tdh: usize,

    /// Transmit descriptor tail
    tdt: usize,
}

// Default is derived only for E1000. Any other registers should set the values
// by themselves
impl Default for NicRegisters {
    fn default() -> Self {
        Self {
            ctrl:  0x0000,
            imc:   0x00D8,
            rctl:  0x0100,
            ral:   0x5400,
            rah:   0x5404,
            rdbal: 0x2800,
            rdbah: 0x2804,
            rdlen: 0x2808,
            rdh:   0x2810,
            rdt:   0x2818,
            tctl:  0x0400,
            tdbal: 0x3800,
            tdbah: 0x3804,
            tdlen: 0x3808,
            tdh:   0x3810,
            tdt:   0x3818,
        }
    }
}

/// Intel NIC receive descriptor
#[allow(missing_docs)]
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
struct RxDescriptor {
    addr:     PhysAddr,
    len:      u16,
    checksum: u16,
    status:   u8,
    errors:   u8,
    special:  u16,
}

/// Intel NIC legacy transmit descriptor
#[allow(missing_docs)]
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
struct LegacyTxDescriptor {
    addr:    PhysAddr,
    len:     u16,
    cso:     u8,
    cmd:     u8,
    status:  u8,
    css:     u8,
    special: u16,
}

/// Transmit state of a NIC
struct TxState {
    /// Virtually mapped TX descriptors
    descs: mm::ContigPageAligned<[LegacyTxDescriptor; TX_DESCS_N]>,

    /// Packets held by the transmit descriptors
    packets: Vec<Option<Packet>>,

    /// Current index of the transmit descriptors that has not yet been sent
    head: usize,

    /// Current index of the transmit descriptors that has been sent
    tail: usize,
}

/// Receive state of a NIC
struct RxState {
    /// Virtually mapped RX descriptors
    descs: mm::ContigPageAligned<[RxDescriptor; RX_DESCS_N]>,

    /// Receive packet buffers corresponding to their descriptors
    packets: Vec<Packet>,

    /// Current index of the receive buffer which is the next one to get a
    /// packet from the NIC
    head: usize,
}

/// The Intel e1000 gigabit network device
pub struct IntelNic {
    /// Register offsets for this NIC
    regs: NicRegisters,

    /// The MMIO space of this NIC
    mmio: &'static mut [u32],

    /// The MAC address of this device
    mac: Mac,

    /// The receive state of the NIC
    rx_state: SpinLock<RxState, InterruptLock>,

    /// The transmit state of the NIC
    tx_state: SpinLock<TxState, InterruptLock>,

    /// A free list of packets, used to avoid packet relocation
    packets: SpinLock<Vec<Packet>, InterruptLock>,
}

impl IntelNic {
    pub fn new(device: DeviceConfig) -> Self {
        // Make sure BAR0 is a memory bar
        assert!(BarType::from_bar(device.bar0) == BarType::Memory,
            "Intel NIC BAR0 not a memory BAR");

        // Get the physical address from the BARs
        let phys_addr = PhysAddr(BarBits::u64(device.bar0, device.bar1));

        // Make sure it's aligned
        assert!(phys_addr.is_aligned_to_page(PageType::Page4K),
            "Intel NIC MMIO not page aligned");

        // Map in the MMIO region into our page tables
        let mmio = {
            // Get a virtual address capable of holding this region
            let vaddr = mm::receive_vaddr_4k(MMIO_SIZE as u64);

            // Acquire acces to physical memory and the page tables
            let mut pmem = mm::PhysicalMemory;
            let mut table = core!().shared.kernel_pt().lock();
            let table = table.as_mut().unwrap();

            // Calculate the end address of the MMIO
            let end_addr = phys_addr.0.checked_add(MMIO_SIZE as u64)
                .expect("Overflow when mapping in Intel NIC MMIO");

            // Map in the MMIO into virtual memory
            let page_size = PageType::Page4K as usize;
            for paddr in (phys_addr.0..end_addr).step_by(page_size) {
                // Compute the offset into MMIO space
                let offset = paddr - phys_addr.0;

                // Map it in
                unsafe {
                    table.map_raw(&mut pmem, VirtAddr(vaddr.0 + offset),
                                  PageType::Page4K,
                                  paddr | PAGE_NXE | PAGE_WRITE |
                                  PAGE_CACHE_DISABLE | PAGE_PRESENT)
                        .expect("Failed to map in Intel NIC MMIO to \
                            virtual memory")
                }
            }

            // Return the MMIO slice
            unsafe {
                core::slice::from_raw_parts_mut(
                    vaddr.0 as *mut u32,
                    (MMIO_SIZE / size_of::<u32>()) as usize)
            }
        };

        // Create the RX descriptor table
        let mut rx_descs = mm::ContigPageAligned::new(
            [RxDescriptor::default(); RX_DESCS_N]);

        // Create the RX packet buffers
        let mut rx_bufs: Vec<Packet> = Vec::with_capacity(RX_DESCS_N);
        for i in 0..rx_bufs.capacity() {
            // Allocate a new packet buffer
            let rx_buf = Packet::new();

            // Store the address of the packet buffer in the descriptor table
            rx_descs[i].addr = rx_buf.phys_addr();

            // Save a ref to the packet buffer
            rx_bufs.push(rx_buf);
        }

        // Create the TX descriptor table
        let tx_descs = mm::ContigPageAligned::new(
            [LegacyTxDescriptor::default(); TX_DESCS_N]);

        // Create the NIC struct
        let mut nic = Self {
            mmio,
            mac: Default::default(),
            regs: Default::default(),
            rx_state: SpinLock::new(RxState {
                descs:   rx_descs,
                packets: rx_bufs,
                head:    0,
            }),
            tx_state: SpinLock::new(TxState {
                descs:   tx_descs,
                packets: (0..TX_DESCS_N).map(|_| None).collect(),
                head:    0,
                tail:    0,
            }),
            packets: SpinLock::new(Vec::with_capacity(TX_DESCS_N + RX_DESCS_N)),
        };

        // Reset the NIC and initialize it for receive and transmit
        unsafe {
            nic.reset();
            nic.init_receive();
            nic.init_transmit();
        }

        // Assign the MAC to the NIC
        nic.mac = nic.read_mac();

        nic
    }

    /// Initialize the NIC for receive
    unsafe fn init_receive(&mut self) {
        let rx_state = self.rx_state.lock();
        unsafe {
            // Set the descriptor base
            self.write_high_low(self.regs.rdbah, self.regs.rdbal,
                rx_state.descs.phys_addr().0);

            // Set the size of the descriptor queue
            self.write(self.regs.rdlen,
                core::mem::size_of_val(&rx_state.descs[..]) as u32);

            // Set the head and tail
            self.write(self.regs.rdh, 0);
            self.write(self.regs.rdt, rx_state.descs.len() as u32 - 1);

            // Enable receive, accept broadcast packets and set the receive
            // buffer size to 4 KiB
            let bits = (1 << 1) | (1 << 15) | (3 << 16) | (1 << 25);
            self.write(self.regs.rctl, bits);
        }
    }

    /// Initialize the NIC for transmit
    unsafe fn init_transmit(&mut self) {
        let tx_state = self.tx_state.lock();
        unsafe {
            // Set the descriptor base
            self.write_high_low(self.regs.tdbah, self.regs.tdbal,
                tx_state.descs.phys_addr().0);

            // Set the size of the descriptor queue
            self.write(self.regs.tdlen,
                core::mem::size_of_val(&tx_state.descs[..]) as u32);

            // Set the head and tail
            self.write(self.regs.tdh, 0);
            self.write(self.regs.tdt, 0);

            // Enable transmit
            self.write(self.regs.tctl, 1 << 1);
        }
    }

    /// Read the receive address for the first entry in the RX MAC filter, which
    /// presumably holds the MAC address of this nic
    fn read_mac(&self) -> Mac {
        let ral = unsafe { self.read(self.regs.ral) };
        let rah = unsafe { self.read(self.regs.rah) };
        assert!((rah & (1 << 31)) != 0, "Couldn't get MAC");
        let mac = [
            ( ral        & 0xFF) as u8,
            ((ral >>  8) & 0xFF) as u8,
            ((ral >> 16) & 0xFF) as u8,
            ((ral >> 24) & 0xFF) as u8,
            ( rah        & 0xFF) as u8,
            ((rah >>  8) & 0xFF) as u8,
        ];
        Mac(mac)
    }

    /// Mask off all of the interrupts
    fn disable_interrupts(&self) {
        unsafe { self.write(self.regs.imc, !0) }
    }

    /// Return a `u64`, reading `rew_high` as the high 32 bits and `reg_low` as
    /// the low 32 bits
    #[allow(unused)]
    unsafe fn read_high_low(&self, reg_high: usize, reg_low: usize) -> u64 {
        unsafe {
            ((self.read(reg_high) as u64) << 32) | self.read(reg_low) as u64
        }
    }

    /// Write the high 32 bits of `val` to `reg_high` and the low 32 bits to
    /// `reg_low`
    unsafe fn write_high_low(&self, reg_high: usize, reg_low: usize, val: u64) {
        unsafe {
            self.write(reg_high, (val >> 32) as u32);
            self.write(reg_low, val as u32);
        }
    }

    /// Read a value from the MMIO register at `reg_offset`
    unsafe fn read(&self, reg_offset: usize) -> u32 {
        let offset = reg_offset / size_of::<u32>();
        unsafe { core::ptr::read_volatile(self.mmio.as_ptr().add(offset)) }
    }

    /// Write a `val` to the MMIO register at `reg_offset`
    unsafe fn write(&self, reg_offset: usize, val: u32) {
        let offset = reg_offset / size_of::<u32>();
        let ptr = self.mmio.as_ptr() as *const u32 as *mut u32;
        unsafe { core::ptr::write_volatile(ptr.add(offset), val); }
    }
}

impl NetDriver for IntelNic {
    unsafe fn reset(&self) {
        unsafe {
            // Mask off all interupts
            self.disable_interrupts();

            // Reset the NIC
            self.write(self.regs.ctrl, 1 << 26);

            // Wait for the reset bit to clear
            while self.read(self.regs.ctrl) & (1 << 26) != 0 {
                core::hint::spin_loop();
            }
            crate::time::sleep(20_000);

            // Mask off all interupts
            self.disable_interrupts();
        }
    }

    fn mac(&self) -> Mac {
        self.mac.clone()
    }

    fn recv<'a: 'b, 'b>(&'a self) -> Option<PacketLease<'b>> {
        // Get unique access to the RX
        let mut rx_state = self.rx_state.lock();
        let head = rx_state.head;
        let desc = &rx_state.descs[head];

        unsafe {
            // Check if there's packet on the line and bail out if not
            if (core::ptr::read_volatile(&desc.status) & 1) == 0 {
                return None;
            }

            // Make sure there's no errors
            assert!(core::ptr::read_volatile(&desc.errors) == 0,
                "RX descriptor error detected");

            // Get the length of the packet
            let len = core::ptr::read_volatile(&desc.len) as usize;

            // Allocate a new packet for this descriptor
            let mut new_packet = self.allocate_packet();
            let phys_addr = new_packet.phys_addr();

            // Swap in the new packet with the old one
            core::mem::swap(&mut new_packet, &mut rx_state.packets[head]);

            // Put this descriptor back for use by the NIC
            core::ptr::write_volatile(
                &mut rx_state.descs[head],
                RxDescriptor { addr: phys_addr, ..Default::default() });

            // Set the tail, letting the NIC know this buffer is available again
            self.write(self.regs.rdt, head as u32);

            // Increment the head
            rx_state.head = (head + 1) % rx_state.descs.len();

            // Set the length of the packet and return a lease to it
            new_packet.set_len(len);
            Some(PacketLease::new(self, new_packet))
        }
    }

    fn send(&self, mut packet: Packet, flush: bool) {
        /// The minimum packet size as specified by the IEEE spec
        const PACKET_MIN_SIZE: usize = 64;

        // Get unique access to the TX
        let mut tx_state = self.tx_state.lock();

        // Pad packet if smaller than minimum size
        if packet.len() < PACKET_MIN_SIZE {
            let len = packet.len();
            let needed = PACKET_MIN_SIZE - len;

            let cursor = packet.cursor();
            let (_, cursor) = cursor.split_at_current();
            let (buf, _) = cursor.split_at(needed);
            buf.fill(0);
        }

        // Wait until there's space in the TX ring
        while tx_state.tail - tx_state.head >= tx_state.descs.len() - 1 {
            // No room in the queue, update the head for each packet which was
            // sent by the NIC previously
            for end in (tx_state.head..tx_state.tail).rev() {
                // Get the status for the queued packet at the head. If the
                // packet has been sent, there's now room for a packet
                let idx = end % tx_state.descs.len();
                let status = unsafe {
                    core::ptr::read_volatile(&tx_state.descs[idx].status)
                };
                if (status & 1) != 0 {
                    tx_state.head = end + 1;
                    break;
                }
            }
        }

        // Fill in the TX descriptor
        let idx = tx_state.tail % tx_state.descs.len();
        tx_state.descs[idx] = LegacyTxDescriptor {
            // Report status, insert FCS, end of packet
            cmd: (1 << 3) | (1 << 1) | (1 << 0),
            addr: packet.phys_addr(),
            len: packet.len() as u16,
            ..Default::default()
        };

        // Swap the new packet into the buffer list
        let mut old_packet = Some(packet);
        core::mem::swap(&mut old_packet, &mut tx_state.packets[idx]);

        // If we replaced an existing packet, free it
        if let Some(old) = old_packet {
            self.release_packet(old);
        }

        // Increment the tail
        tx_state.tail = tx_state.tail.wrapping_add(1);

        // Flush if we should
        if flush || (tx_state.tail-tx_state.head) == (tx_state.descs.len()-1) {
            unsafe {
                self.write(
                    self.regs.tdt,
                    (tx_state.tail % tx_state.descs.len()) as u32);
            }
        }
    }

    fn allocate_packet(&self) -> Packet {
        self.packets.lock().pop().unwrap_or_else(|| Packet::new())
    }

    fn release_packet(&self, mut packet: Packet) {
        let mut packets = self.packets.lock();
        if packets.len() < packets.capacity() {
            packet.clear();
            packets.push(packet)
        }
    }
}

impl Device for IntelNic {
    fn purge(&self) {
        unsafe { self.reset() }
    }
}

// Register the probe function for this driver
crate::register_pci_driver!(probe);
fn probe(cfg: &DeviceConfig) -> Option<Arc<dyn Device>> {
    // The PCI IDs this driver can handle
    let (vid, did) = (0x8086, 0x100E);

    // If this device matches our IDs, register it
    if (vid, did) == (cfg.header.vendor_id, cfg.header.device_id) {
        // Create the driver
        let driver = Arc::new(IntelNic::new(*cfg));

        // Register it as a net device
        NetDevice::register(driver.clone());

        // Return it as a PCI device
        return Some(driver);
    }

    None
}
