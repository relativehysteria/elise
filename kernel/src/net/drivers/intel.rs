//! Intel e1000/e drivers

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
use crate::net::{NetDriver, Mac};
use crate::core_locals::InterruptLock;

/// The Intel NICs map in 128KiB of memory
const MMIO_SIZE: usize = 128 * 1024;

// Make sure the descriptor tables fit on a single page. Each descriptor is
// 16-bytes in size, so there should be at most 4096 / 16 descriptors, which is
// 256, or u8::MAX.
// They must also be 128-byte aligned, so that's 128 / 16 = 8 descs.

/// Number of receive descriptors to allocate for a NIC
const RX_DESCS_N: usize = 256;
const_assert!(RX_DESCS_N <= 256);
const_assert!(RX_DESCS_N % 8 == 0);

/// Number of transmit descriptors to allocate for a NIC
const TX_DESCS_N: usize = 256;
const_assert!(TX_DESCS_N <= 256);
const_assert!(TX_DESCS_N % 8 == 0);

#[derive(Clone, Copy)]
/// NIC register offsets
struct NicRegisters {
    /// Device control
    ctrl: usize,

    /// Interrupt mask clear
    imc: usize,

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
            ral:   0x5400,
            rah:   0x5404,
            rdbal: 0x2800,
            rdbah: 0x2804,
            rdlen: 0x2808,
            rdh:   0x2810,
            rdt:   0x2818,
            tdbal: 0x3800,
            tdbah: 0x3804,
            tdlen: 0x3808,
            tdh:   0x3810,
            tdt:   0x3818,
        }
    }
}

#[allow(missing_docs)]
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
/// Intel NIC receive descriptor
struct RxDescriptor {
    addr:     u64,
    len:      u16,
    checksum: u16,
    status:   u8,
    errors:   u8,
    special:  u16,
}

#[allow(missing_docs)]
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
/// Intel NIC legacy transmit descriptor
struct LegacyTxDescriptor {
    addr:    u64,
    len:     u16,
    cso:     u8,
    cmd:     u8,
    status:  u8,
    rsv:     u8,
    css:     u8,
    special: u16,
}

/// Allocated packet that can be put into and taken from DMA buffers.
///
/// The inner backing buffer is guaranteed to be page-aligned.
struct Packet {
    /// The raw backing memory for the packet
    raw: mm::ContigPageAligned<[u8; 4096]>,

    /// Size of the inner backing memory
    length: usize,
}

impl Packet {
    /// Allocate a new packet buffer
    fn new() -> Self {
        Self {
            raw: mm::ContigPageAligned::new([0u8; 4096]),
            length: 0,
        }
    }
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

/// The Intel gigabit network device
struct IntelNic {
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
}

impl IntelNic {
    fn new(device: DeviceConfig) -> Self {
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
            rx_descs[i].addr = rx_buf.raw.phys_addr().0;

            // Save a ref to the packet buffer
            rx_bufs.push(rx_buf);
        }

        // Create the TX descriptor table
        let tx_descs = mm::ContigPageAligned::new(
            [LegacyTxDescriptor::default(); TX_DESCS_N]);

        // Create the NIC struct
        let mut nic = Self {
            mmio,
            mac: Mac([0; 6]),
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
            })
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
        }
    }

    /// Initialize the NIC for transmit
    unsafe fn init_transmit(&mut self) {
        let tx_state = self.tx_state.lock();
        unsafe {
            // Set the descriptor base
            self.write_high_low(self.regs.rdbah, self.regs.rdbal,
                tx_state.descs.phys_addr().0);

            // Set the size of the descriptor queue
            self.write(self.regs.tdlen,
                core::mem::size_of_val(&tx_state.descs[..]) as u32);

            // Set the head and tail
            self.write(self.regs.tdh, 0);
            self.write(self.regs.tdt, 0);
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
}

impl Device for IntelNic {
    fn purge(&self) {
        unsafe { self.reset() }
    }
}

// Register the probe function for this driver
crate::register_pci_driver!(probe);
fn probe(cfg: &DeviceConfig) -> Option<Arc<dyn Device>> {
    let (vid, did) = (0x8086, 0x100E);
    if (vid, did) == (cfg.header.vendor_id, cfg.header.device_id) {
        return Some(Arc::new(IntelNic::new(*cfg)));
    }

    None
}
