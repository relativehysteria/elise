/// Macro that checks whether a value is 4K aligned at compile time
macro_rules! is_4k_aligned {
    ($x:expr) => {
        const_assert::const_assert!(($x & (!(0x1000 - 1))) == $x);
    }
}

/// Macro that checks whether a value is 1GiB aligned at compile time
macro_rules! is_1g_aligned {
    ($x:expr) => {
        const_assert::const_assert!(($x & (!(0x4000_0000- 1))) == $x);
    }
}

/// Macro that rounds a value down to the nearest multiple of a given alignment.
macro_rules! floor_align {
    ($val:expr, $align:expr) => {
        ($val / $align) * $align
    }
}

// Memory map:
// |---------- kernel code
// |---------- shared data
// |---------- trampoline
// |
// |---------- phys window
// |---------- vmem alloc base
// |---------- kernel stacks

/// The base at which the kernel code will be loaded.
///
/// This is the value in kernel/.cargo/config.toml
pub const KERNEL_CODE_BASE: u64 = 0xFFFF_FFFF_CAFE_0000;

/// The base address to use for the trampoline code that is present both in the
/// bootloader and in the kernel page tables.
///
/// The trampoline is a small piece of code that acts as a both-way transition
/// between the bootloader and the kernel (such that the bootloader can jump to
/// the kernel and the kernel to the bootloader).
pub const TRAMPOLINE_ADDR: u64 =
    KERNEL_CODE_BASE - (REGION_PADDING + MAX_TRAMPOLINE_SIZE);

/// This is the maximum size in bytes we allow the trampoline code to be;
pub const MAX_TRAMPOLINE_SIZE: u64 = 0x10_000;

/// The virtual base in the kernel page tables where physical memory is
/// linearly mapped, such that dereference of `KERNEL_PHYS_WINDOW_BASE` will be
/// accessing `0` in physical memory.
///
/// This must be 1GiB-aligned for large-page mapping.
pub const KERNEL_PHYS_WINDOW_BASE: u64 = floor_align!(
    TRAMPOLINE_ADDR - (KERNEL_PHYS_WINDOW_SIZE + REGION_PADDING),
    0x4000_0000);

/// Size of the kernel physical window (in bytes)
pub const KERNEL_PHYS_WINDOW_SIZE: u64 = 0x100_0000_0000;

/// The base virtual address that will be used for dynamic virtual allocations
pub const KERNEL_VMEM_BASE: u64 =
    KERNEL_PHYS_WINDOW_BASE - (KERNEL_VMEM_SIZE + REGION_PADDING);

/// Size of the kernel virtual arena (in bytes)
pub const KERNEL_VMEM_SIZE: u64 = 0x100_0000_0000;

/// The base address to use for the kernel stacks.
///
/// This will be the address for the first allocated kernel stack. Addresses
/// from this base grow downwards.
pub const KERNEL_STACK_BASE: u64 = KERNEL_VMEM_BASE - REGION_PADDING;

/// Size to allocate for kernel stacks.
pub const KERNEL_STACK_SIZE: u64 = 128 * 0x1000;

/// Padding space to add between memory regions to prevent overwrites and such.
pub const REGION_PADDING: u64 = 8 * 0x1000;

/// The size of the whole stack together with its padding
pub const KERNEL_STACK_SIZE_PADDED: u64 = KERNEL_STACK_SIZE + REGION_PADDING;

// Validate all of the constants
is_4k_aligned!(KERNEL_CODE_BASE);
is_4k_aligned!(TRAMPOLINE_ADDR);
is_4k_aligned!(KERNEL_STACK_BASE);
is_4k_aligned!(KERNEL_STACK_SIZE_PADDED);
is_4k_aligned!(REGION_PADDING);
is_4k_aligned!(KERNEL_VMEM_BASE);
is_1g_aligned!(KERNEL_PHYS_WINDOW_BASE);
