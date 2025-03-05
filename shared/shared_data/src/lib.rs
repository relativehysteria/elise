//! Common structure for data that is shared between the bootloader and the
//! kernel.

#![no_std]

use spinlock::SpinLock;
use serial::SerialDriver;
use rangeset::RangeSet;

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
    pub free_memory: SpinLock<Option<RangeSet>>,
}

impl Shared {
    /// Creates an empty structure for shared data
    pub const fn new() -> Self {
        Self {
            serial: SpinLock::new(None),
            free_memory: SpinLock::new(None),
        }
    }
}
