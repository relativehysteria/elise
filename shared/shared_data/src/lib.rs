//! Common structure for data that is shared between the bootloader and the
//! kernel.

#![no_std]

use core::sync::atomic::AtomicU64;
use spinlock::SpinLock;
use oncelock::OnceLock;
use serial::SerialDriver;
use rangeset::RangeSet;
use elf_parser::Elf;
use page_table::{PageTable, VirtAddr};

/// Macro that checks whether a value is 4K aligned at compile time
macro_rules! is_4k_aligned {
    ($x:expr) => {
        const_assert::const_assert!(($x & (!(4096 - 1))) == $x);
    }
}

/// The base at which the kernel code will be loaded.
///
/// This is the value in kernel/.cargo/config.toml
pub const KERNEL_CODE_BASE: u64 = 0xFFFF_FFFF_CAFE_0000;

/// The base at which the SHARED data structure will be loaded
pub const KERNEL_SHARED_BASE: u64 = KERNEL_CODE_BASE - 0x1_0000;

/// The base address to use for the trampoline code that is present both in the
/// bootloader and in the kernel page tables.
///
/// The trampoline is a small piece of code that transistions from the
/// bootloader page table into the kernel page table before jumping to the
/// kernel.
pub const TRAMPOLINE_ADDR: u64 = KERNEL_SHARED_BASE - 0x1_0000;

/// The base address to use for the kernel stacks for the first core.
pub const KERNEL_STACK_BASE: u64 = TRAMPOLINE_ADDR - 0x1_0000;

/// Size to allocate for kernel stacks.
pub const KERNEL_STACK_SIZE: u64 = 128 * 0x1000;

/// Padding space to add between kernel stacks to prevent overwrites and such.
pub const KERNEL_STACK_PAD: u64 = 8 * 0x1000;
// XXX: Maybe have unmapped guard pages instead of padding?

/// The size of the whole stack together with its padding
pub const KERNEL_STACK_SIZE_PADDED: u64 = KERNEL_STACK_SIZE + KERNEL_STACK_PAD;

// Validate all of the constants
is_4k_aligned!(KERNEL_CODE_BASE);
is_4k_aligned!(KERNEL_SHARED_BASE);
is_4k_aligned!(TRAMPOLINE_ADDR);
is_4k_aligned!(KERNEL_STACK_BASE);
is_4k_aligned!(KERNEL_STACK_SIZE_PADDED);

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

    /// The physical memory map state
    pub free_memory: RangeSet,
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
}
