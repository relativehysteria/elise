//! A core-exclusive data structure which can be accessed via the `core!()`
//! macro.

use core::sync::atomic::{AtomicU32, Ordering};
use core::alloc::Layout;
use page_table::VirtAddr;
use shared_data::{Shared, KERNEL_SHARED_BASE};
use spinlock::SpinLock;
use crate::mm::FreeList;
use crate::interrupts::Interrupts;
use crate::apic::Apic;

/// The value in `CoreLocals.apic_id` if the APIC is uninitialized
const APIC_UNINIT: u32 = u32::MAX;

#[allow(dead_code)]
#[repr(C)]
/// Core local data
pub struct CoreLocals {
    /// The address of this structure
    ///
    /// This address MUST BE THE FIRST field of this struct because the
    /// `core!()` macro relies on it
    address: VirtAddr,

    /// A unique identifier allocated for this core
    pub id: u32,

    /// Data shared between the bootloader and the kernel
    pub shared: &'static Shared,

    /// An initialized APIC implementation. `None` until initialized.
    apic: SpinLock<Option<Apic>>,

    /// Local APIC id.
    apic_id: AtomicU32,

    /// Implementation of interrupts. Used to add interrupt handlers to the
    /// interrupt table. `None` until initialized.
    interrupts: SpinLock<Option<Interrupts>>,

    /// Free lists for each power-of-two size.
    /// The free list size is `(1 << (idx + 3))`
    free_lists: [SpinLock<FreeList>; 61],
}

impl CoreLocals {
    /// Get a free list which can satisfy `layout`
    pub unsafe fn free_list(&self, layout: Layout) -> &SpinLock<FreeList> {
        // The minimum freelist allocation is 8 bytes. Round up if needed
        let size = core::cmp::max(layout.size(), 8);

        // Round up size to the nearest power of two and get the log2 of it
        // to determine the index into the free lists
        let idx = 64 - (size - 1).leading_zeros();

        // Compute the alignment of the free list associated with this memory.
        // Free lists are naturally aligned until 4096 byte sizes, at which
        // point they remain only 4096 byte aligned
        let free_list_align = 1 << core::cmp::min(idx, 12);
        assert!(free_list_align >= layout.align(),
            "Cannot satisfy alignment requirement from free list");

        // Get the free list corresponding to this size.
        // idx gives log2(size) + 1, but the array starts at 8 bytes (idx 3 in
        // log2 scale), so adjust for the offset
        &self.free_lists[idx as usize - 3]
    }

    /// Get access to the interrupt table
    pub unsafe fn interrupts(&self) -> &SpinLock<Option<Interrupts>> {
        &self.interrupts
    }

    /// Get access to the local APIC
    pub unsafe fn apic(&self) -> &SpinLock<Option<Apic>> {
        &self.apic
    }

    /// Set the current core's APIC ID
    pub unsafe fn set_apic_id(&self, apic_id: u32) {
        self.apic_id.store(apic_id, Ordering::SeqCst);
    }

    /// Get access to the current core's APIC ID if initialized
    pub unsafe fn apic_id(&self) -> Option<u32> {
        match self.apic_id.load(Ordering::SeqCst) {
            APIC_UNINIT => None,
            x @ _       => Some(x),
        }
    }

    /// Get the preferred memory range for the currently running core.
    /// Returns `None` if there's no valid APIC ID or we have no knowledge of
    /// NUMA.
    pub fn mem_range<'a>(&self) -> Option<&'a rangeset::RangeSet> {
        crate::mm::mem_range()
    }
}

/// Returns a reference to current core locals
#[macro_export] macro_rules! core {
    () => {
        $crate::core_locals::get_core_locals()
    }
}

#[inline]
/// Returns a reference to the data local to this core
pub fn get_core_locals() -> &'static CoreLocals {
    // Get the first `u64` from `CoreLocals`, which should be the address
    unsafe {
        let ptr: usize;
        core::arch::asm!(
            "mov {0}, gs:[0]",
            out(reg) ptr,
            options(nostack, preserves_flags));
        &*(ptr as *const CoreLocals)
    }
}

/// Initialize the core locals for this core
pub fn init(core_id: u32) {
    // Get a reference to the SHARED structure
    let shared = unsafe { &*(KERNEL_SHARED_BASE as *const Shared) };

    // Allocate space for the core locals
    let core_locals_ptr = {
        let mut pmem = shared.free_memory().lock();
        let pmem = pmem.as_mut().unwrap();

        pmem.allocate(
            core::mem::size_of::<CoreLocals>() as u64,
            core::mem::align_of::<CoreLocals>() as u64
        ).unwrap().unwrap() + shared_data::KERNEL_PHYS_WINDOW_BASE
    };

    macro_rules! generate_freelists {
        ($($size:expr),*) => {
            [
                $(
                    SpinLock::new(FreeList::new($size)),
                )*
            ]
        };
    }

    // Create the struct
    let locals = CoreLocals {
        address:    VirtAddr(core_locals_ptr),
        id:         core_id,
        shared:     shared,
        apic:       SpinLock::new(None),
        apic_id:    AtomicU32::new(APIC_UNINIT),
        interrupts: SpinLock::new(None),
        free_lists: generate_freelists!(
            0x0000000000000008, 0x0000000000000010, 0x0000000000000020,
            0x0000000000000040, 0x0000000000000080, 0x0000000000000100,
            0x0000000000000200, 0x0000000000000400, 0x0000000000000800,
            0x0000000000001000, 0x0000000000002000, 0x0000000000004000,
            0x0000000000008000, 0x0000000000010000, 0x0000000000020000,
            0x0000000000040000, 0x0000000000080000, 0x0000000000100000,
            0x0000000000200000, 0x0000000000400000, 0x0000000000800000,
            0x0000000001000000, 0x0000000002000000, 0x0000000004000000,
            0x0000000008000000, 0x0000000010000000, 0x0000000020000000,
            0x0000000040000000, 0x0000000080000000, 0x0000000100000000,
            0x0000000200000000, 0x0000000400000000, 0x0000000800000000,
            0x0000001000000000, 0x0000002000000000, 0x0000004000000000,
            0x0000008000000000, 0x0000010000000000, 0x0000020000000000,
            0x0000040000000000, 0x0000080000000000, 0x0000100000000000,
            0x0000200000000000, 0x0000400000000000, 0x0000800000000000,
            0x0001000000000000, 0x0002000000000000, 0x0004000000000000,
            0x0008000000000000, 0x0010000000000000, 0x0020000000000000,
            0x0040000000000000, 0x0080000000000000, 0x0100000000000000,
            0x0200000000000000, 0x0400000000000000, 0x0800000000000000,
            0x1000000000000000, 0x2000000000000000, 0x4000000000000000,
            0x8000000000000000
        ),
    };

    unsafe {
        // Write the struct to the allocation
        core::ptr::write(core_locals_ptr as *mut CoreLocals, locals);

        // Set GS so we can access the locals from anywhere using `core!()`
        cpu::set_gs_base(core_locals_ptr as u64);
    }
}
