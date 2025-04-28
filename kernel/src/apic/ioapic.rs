//! IO APIC implementation

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU8, Ordering};

use page_table::{
    PhysAddr, PageType, PAGE_NXE, PAGE_WRITE, PAGE_CACHE_DISABLE, PAGE_PRESENT};
use oncelock::OnceLock;
use rangeset::Range;

use crate::acpi::IsaSourceOverrides;

/// All of the IO APICs on the system.
///
/// This vector is sorted by the range of GSIs each IO APIC handles.
static IOAPICS: OnceLock<&[IoApic]> = OnceLock::new();

/// Mapping of GSIs to their respective IO APIC.
///
/// Index into this vector is the GSI and the resulting value is the IO APIC id
/// (index into `IOAPICS`)
static GSI_TO_IOAPIC: OnceLock<&[usize]> = OnceLock::new();

/// Current mapping of GSIs to their respective IDT, offset by 0x20.
static GSI_TO_IDT: OnceLock<&[AtomicU8]> = OnceLock::new();

#[derive(Clone, Copy)]
#[repr(u8)]
/// IO APIC registers (offsets into MMIO space)
pub enum Register {
    /// ID register
    Id = 0x00,

    /// Version register
    Ver = 0x01,

    /// Arbitration register
    Arb = 0x02,
}

#[derive(Debug)]
/// An IO APIC that has yet to be initialized
pub struct Uninitialized {
    /// The ID of this IO APIC
    id: u8,

    /// The physical address where the MMIO will be mapped
    addr: PhysAddr,

    /// The first GSI this IO APIC will handle
    gsi_base: u32,
}

impl Uninitialized {
    /// Return a new uninitialized IO APIC
    pub fn new(id: u8, addr: PhysAddr, gsi_base: u32) -> Self {
        Self { id, addr, gsi_base }
    }

    /// Initialize this IO APIC
    pub fn init(self) -> IoApic {
        IoApic::new_init(self.id, self.addr, self.gsi_base)
    }
}

#[derive(Debug)]
/// IO APIC
pub struct IoApic {
    /// The ID of this IO APIC
    id: u8,

    /// The MMIO mapping of this APIC
    mmio: &'static mut [u32],

    /// The Global System Interrupts that are handled by this IO APIC.
    gsi: Range,
}

impl IoApic {
    /// Register Select offset into mmio
    const REG_SEL: usize = 0x00;

    /// Register Window offset into mmio
    const REG_WIN: usize = 0x10;

    /// Creates and initializes new IO APIC, mapping it into memory.
    fn new_init(id: u8, addr: PhysAddr, gsi_base: u32) -> Self {
        // Get a virtual address for this IO APIC
        let vaddr = crate::mm::receive_vaddr_4k(4096);

        // Get access to the current page table
        let mut pmem = crate::mm::PhysicalMemory;
        let mut table = core!().shared.kernel_pt().lock();
        let table = table.as_mut().unwrap();

        // Map the IO APIC into memory
        let mapping = unsafe {
            table.map_raw(&mut pmem, vaddr, PageType::Page4K,
                addr.0 | PAGE_NXE | PAGE_WRITE |
                PAGE_CACHE_DISABLE | PAGE_PRESENT)
            .expect("Couldn't the IO APIC into virtual memory");

            // Convert the memory into a rust slice
            core::slice::from_raw_parts_mut(vaddr.0 as *mut u32, 1024)
        };

        // Create the IoApic struct. For now, put a placeholder value into GSI
        let mut ioapic = IoApic {
            id,
            mmio: mapping,
            gsi: Range::new(0, 0).unwrap(),
        };

        // Get the number of GSIs handled by this IO APIC
        let gsi_n = unsafe { (ioapic.read(Register::Ver) >> 16) & 0xFF };
        let gsi_max = gsi_base.checked_add(gsi_n).expect("GSI entry overflow");

        // Set the actual GSI range of this IO APIC
        ioapic.gsi = Range::new(gsi_base as u64, gsi_max as u64).unwrap();

        // Return it
        ioapic
    }

    /// Read from an IO APIC `register`
    pub unsafe fn read(&mut self, register: Register) -> u32 {
        unsafe { self.read_raw(register as u8) }
    }

    /// Write into an IO APIC `register`
    pub unsafe fn write(&mut self, register: Register, val: u32) {
        unsafe { self.write_raw(register as u8, val); }
    }

    #[inline]
    /// Return the pointer to the selector
    unsafe fn sel(&mut self) -> *mut u32 {
        unsafe { self.mmio.as_mut_ptr().byte_add(Self::REG_SEL) }
    }

    #[inline]
    /// Return the pointer to the window
    unsafe fn win(&mut self) -> *mut u32 {
        unsafe { self.mmio.as_mut_ptr().byte_add(Self::REG_WIN) }
    }

    /// Read from a raw offset into MMIO
    unsafe fn read_raw(&mut self, offset: u8) -> u32 {
        unsafe {
            // Tell selector where we wanna read from and read from the window
            core::ptr::write_volatile(self.sel(), offset as u32);
            core::ptr::read_volatile(self.win())
        }
    }

    /// Write to a raw offset into MMIO
    unsafe fn write_raw(&mut self, offset: u8, val: u32) {
        unsafe {
            // Tell selector where we wanna read from and write into the window
            core::ptr::write_volatile(self.sel(), offset as u32);
            core::ptr::write_volatile(self.win(), val)
        }
    }

    /// Read the redirection `entry` (0-23)
    pub unsafe fn read_redir(&mut self, entry: u8) -> u64 {
        let lower = unsafe { self.read_raw(0x10 + 0 + 2 * entry) as u64 };
        let upper = unsafe { self.read_raw(0x10 + 1 + 2 * entry) as u64 };
        (upper << 32) + lower
    }

    /// Write the redirection `entry` (0-23)
    pub unsafe fn write_redir(&mut self, entry: u8, val: u64) {
        unsafe {
            self.write_raw(0x10 + 0 + 2 * entry, (val & 0xFFFF) as u32);
            self.write_raw(0x10 + 1 + 2 * entry, (val >> 32) as u32);
        }
    }
}

/// Initialize the IO APICs on the system
pub fn init(io_apics: Vec<Uninitialized>, overrides: IsaSourceOverrides) {
    // Initialize IO APICs and collect them by ID
    let mut ioapics = Vec::with_capacity(io_apics.len());
    let mut max_gsi = 0;

    // Initialize IO APICs and keep track of the maximum ID and GSI handled
    for uninit in io_apics {
        let ioapic = uninit.init();
        max_gsi = max_gsi.max(ioapic.gsi.end());
        ioapics.push(ioapic);
    }

    // Sort the IO APICs by their GSI base
    ioapics.sort_unstable_by_key(|apic| apic.gsi.start());

    // Check the GSI ranges for overlaps
    for pair in ioapics.windows(2) {
        let a = &pair[0];
        let b = &pair[1];
        if a.gsi.overlaps(&b.gsi).is_some() {
            panic!("GSI overlap detected between IO APICs {} and {}: {:?} {:?}",
                a.id, b.id, a.gsi, b.gsi);
        }
    }

    // Build GSI to IOAPIC index mapping
    let mut gsi_to_ioapic: Vec<_> = (0..max_gsi).map(|_| None).collect();
    for (idx, ioapic) in ioapics.iter().enumerate() {
        for gsi in ioapic.gsi.start()..ioapic.gsi.end() {
            gsi_to_ioapic[gsi as usize] = Some(idx);
        }
    }

    // Verify no GSI is unhandled
    let unhandled_gsi = gsi_to_ioapic.iter()
        .enumerate()
        .find(|(_, v)| v.is_none())
        .map(|(i, _)| i);
    if let Some(gsi) = unhandled_gsi {
        panic!("GSI {gsi} is not handled by any IO APIC!");
    }

    // Flatten the mapping
    let gsi_to_ioapic: Vec<_> = gsi_to_ioapic.into_iter()
        .map(Option::unwrap)
        .collect();

    // Now create the GSI to IDT mapping. Do 1:1 by default
    let mut gsi_to_ivt: Vec<AtomicU8> = (0..max_gsi)
        .map(|gsi| AtomicU8::new(gsi as u8))
        .collect();

    // Save the mappings
    GSI_TO_IOAPIC.set(gsi_to_ioapic.leak());
    IOAPICS.set(ioapics.leak());
}
