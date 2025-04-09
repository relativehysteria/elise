//! Common structure for data that is shared between the bootloader and the
//! kernel.

#![no_std]

extern crate alloc;

mod constants;
mod trampoline;
pub use constants::*;
pub use trampoline::*;

use core::sync::atomic::AtomicU64;
use spinlock::SpinLock;
use oncelock::OnceLock;
use serial::SerialDriver;
use rangeset::RangeSet;
use elf_parser::Elf;
use page_table::{PageTable, VirtAddr, PhysAddr};

#[derive(Debug, Clone)]
/// Information about the state of the bootloader. All virtual addresses are
/// only valid within the bootloader page table.
///
/// This struct is a state snapshot _after_ the trampoline has been mapped in,
/// but _before_ the kernel was mapped in. This allows us to restore the
/// bootloader physical memory and its virtual mappings to a sane state before
/// mapping in the kernel and jumping to it again.
pub struct BootloaderState {
    /// The bootloader page table
    pub page_table: PageTable,

    /// Entry point to the bootloader
    pub entry: VirtAddr,

    /// Virtual address of the bootloader stack
    pub stack: VirtAddr,
}

#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct SdtTable {
    /// Number of SDTs in the SDT table
    pub n_entries: usize,

    /// Unaligned physical address of the first SDT
    pub base: PhysAddr,
}

/// Data structure shared between the kernel and the bootloader
pub struct Shared {
    /// The serial driver that can be used by the kernel and the bootloader to
    /// print messages through the serial ports
    pub serial: SpinLock<Option<SerialDriver>>,

    /// All memory which is available for use by the bootloader and the kernel,
    /// at the same time.
    ///
    /// This memory is acquired through the `get_memory_map()` UEFI boot service
    /// and because UEFI sets up the bootloader paging structures to an identity
    /// map, all pointers in this memory point to valid physical memory even if
    /// paging in the bootloader is enabled (as long as it's the one provided by
    /// UEFI).
    free_memory: SpinLock<Option<RangeSet>>,

    /// Physical address of where the kernel image to boot is present.
    ///
    /// If this is `None`, the kernel image embedded within the bootloader will
    /// be booted instead of this.
    kernel_image: SpinLock<Option<Elf<'static>>>,

    /// The page table used for the kernel
    kernel_pt: SpinLock<Option<PageTable>>,

    /// The virtual address of the next available stack.
    next_stack: AtomicU64,

    /// The table of other ACPI SDTs
    ///
    /// As dictated by the UEFI spec, we have to retrieve this pointer before we
    /// exit the boot services, but because of memory constraints, we will parse
    /// ACPI tables on the kernel side.
    acpi_sdt: OnceLock<SdtTable>,

    /// A snapshot of the bootloader after the bootloader has been initialized
    /// to its permanent state.
    bootloader: OnceLock<BootloaderState>,
}

impl Shared {
    /// Creates an empty structure for shared data
    pub const fn new() -> Self {
        Self {
            serial:       SpinLock::new(None),
            free_memory:  SpinLock::new(None),
            kernel_image: SpinLock::new(None),
            kernel_pt:    SpinLock::new(None),
            next_stack:   AtomicU64::new(KERNEL_STACK_BASE),
            acpi_sdt:     OnceLock::new(),
            bootloader:   OnceLock::new(),
        }
    }

    /// Returns a reference to the free memory lock
    pub fn free_memory(&self) -> &SpinLock<Option<RangeSet>> {
        &self.free_memory
    }

    /// Returns a reference to the kernel image pointer lock
    pub fn kernel_image(&self) -> &SpinLock<Option<Elf<'static>>> {
        &self.kernel_image
    }

    /// Returns a reference to the kernel page table
    pub fn kernel_pt(&self) -> &SpinLock<Option<PageTable>> {
        &self.kernel_pt
    }

    /// Returns a reference to the bootloader snapshot
    pub fn bootloader(&self) -> &OnceLock<BootloaderState> {
        &self.bootloader
    }

    /// Returns a reference to the base of the next stack
    pub fn stack(&self) -> &AtomicU64 {
        &self.next_stack
    }

    /// Returns a reference to the root ACPI table as given by UEFI
    pub fn acpi(&self) -> &OnceLock<SdtTable> {
        &self.acpi_sdt
    }
}
