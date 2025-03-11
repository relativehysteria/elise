//! Common structure for data that is shared between the bootloader and the
//! kernel.

#![no_std]

use spinlock::SpinLock;
use serial::SerialDriver;
use rangeset::RangeSet;
use elf_parser::Elf;
use page_table::{PageTable, VirtAddr};
use core::sync::atomic::AtomicU64;

/// The base at which the kernel code will be loaded.
///
/// This is the value in kernel/.cargo/config.toml
pub const KERNEL_CODE_BASE: u64 = 0xFFFF_FFFF_CAFE_0000;

/// The base address to use for the trampoline code that is present both in the
/// bootloader and in the kernel page tables.
///
/// The trampoline is a small piece of code that transistions from the
/// bootloader page table into the kernel page table before jumping to the
/// kernel.
pub const TRAMPOLINE_ADDR: u64 = KERNEL_CODE_BASE - 0x38_0000;

/// The base address to use for the kernel stacks for each core.
///
/// This will be the address of the first stack. Other stack addresses will be
/// __below__ this one.
pub const KERNEL_STACK_BASE: u64 = KERNEL_CODE_BASE - 0x40_0000;

/// Size to allocate for kernel stacks.
pub const KERNEL_STACK_SIZE: u64 = 128 * 0x1000;

/// Padding space to add between kernel stacks to prevent overwrites and such.
pub const KERNEL_STACK_PAD: u64 = 8 * 0x1000;
// XXX: Maybe have unmapped guard pages instead of padding?


/// Makes sure that all constants that are required to be aligned are so
pub fn validate_constants() {
    let p = page_table::PageType::Page4K;
    assert!(VirtAddr(KERNEL_CODE_BASE).is_aligned(p));
    assert!(VirtAddr(TRAMPOLINE_ADDR).is_aligned(p));
    assert!(VirtAddr(KERNEL_STACK_BASE).is_aligned(p));
    assert!(VirtAddr(KERNEL_STACK_SIZE.checked_sub(KERNEL_STACK_PAD).unwrap())
        .is_aligned(p))
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

    /// Entry point of the bootloader (0 means uninitialized)
    bootloader_entry: AtomicU64,

    /// The page table used for the bootloader
    bootloader_pt: SpinLock<Option<PageTable>>,
}

impl Shared {
    /// Creates an empty structure for shared data
    pub const fn new() -> Self {
        Self {
            serial:           SpinLock::new(None),
            free_memory:      SpinLock::new(None),
            kernel_image:     SpinLock::new(None),
            kernel_pt:        SpinLock::new(None),
            next_stack:       AtomicU64::new(KERNEL_STACK_BASE),
            bootloader_entry: AtomicU64::new(0),
            bootloader_pt:    SpinLock::new(None),
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

    /// Returns a reference to the bootloader page table
    pub fn bootloader_pt(&self) -> &SpinLock<Option<PageTable>> {
        &self.bootloader_pt
    }

    /// Returns a reference to the bootloader entry point address
    pub fn bootloader_entry(&self) -> &AtomicU64 {
        &self.bootloader_entry
    }

    /// Returns a reference to the next available stack virtual address
    pub fn next_stack(&self) -> &AtomicU64 {
        &self.next_stack
    }
}
