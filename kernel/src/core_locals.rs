//! A core-exclusive data structure which can be accessed via the `core!()`
//! macro.

use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use core::alloc::Layout;

use page_table::VirtAddr;
use shared_data::Shared;
use spinlock::{SpinLock, InterruptState, DummyInterruptState};
use oncelock::OnceLock;
use autorefcount::{AutoRefCount, AutoRefCountGuard};

use crate::mm::FreeList;
use crate::interrupts::Interrupts;
use crate::apic::LocalApic;

/// The cumulative variable used for allocating core IDs
static NEXT_CORE_ID: AtomicU32 = AtomicU32::new(0);

/// Returns a reference to current core locals
#[macro_export] macro_rules! core {
    () => {
        $crate::core_locals::get_core_locals()
    }
}

/// A struct that implements `InterruptState`
pub struct InterruptLock;

impl InterruptState for InterruptLock {
    fn in_interrupt() -> bool { core!().in_interrupt() }
    fn in_exception() -> bool { core!().in_exception() }
    fn enter_lock()           { unsafe { core!().disable_interrupts(); } }
    fn exit_lock()            { unsafe { core!().enable_interrupts(); } }
}

/// Core local data
#[allow(dead_code)]
#[repr(C)]
pub struct CoreLocals {
    /// The address of this structure
    ///
    /// This address MUST BE THE FIRST field of this struct because the
    /// `core!()` macro relies on it
    address: VirtAddr,

    /// A unique identifier allocated for this core
    pub id: u32,

    /// Data shared between the bootloader and the kernel
    pub shared: &'static Shared<InterruptLock>,

    /// An initialized APIC implementation. `None` until initialized.
    apic: SpinLock<Option<LocalApic>, InterruptLock>,

    /// Local APIC id.
    apic_id: OnceLock<u32>,

    /// The number of requests to have interrupts disabled.
    ///
    /// While this value is non-zero, interrupts will be disabled.
    interrupt_disable_requests: AtomicUsize,

    /// Current level of interrupt nesting.
    interrupt_depth: AutoRefCount,

    /// Current level of exception nesting.
    ///
    /// Exceptions are unique in that they may occur while a `no_preempt` lock
    /// is held. Code which may run during an exception must be aware of this
    /// and _must not_ use blocking lock operations.
    exception_depth: AutoRefCount,

    /// Implementation of interrupts. Used to add interrupt handlers to the
    /// interrupt table. `None` until initialized.
    interrupts: SpinLock<Option<Interrupts>, InterruptLock>,

    /// Free lists for each power-of-two size.
    /// The free list size is `(1 << (idx + 3))`
    free_lists: [SpinLock<FreeList, InterruptLock>; 61],
}

impl CoreLocals {
    /// Get a free list which can satisfy `layout`
    pub fn free_list(&self, layout: Layout)
            -> &SpinLock<FreeList, InterruptLock> {
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

    /// Returns whether this core is the bootstrap processor
    pub fn is_bsp(&self) -> bool {
        self.id == 0
    }

    /// Get access to the interrupt table
    pub fn interrupts(&self) -> &SpinLock<Option<Interrupts>, InterruptLock> {
        &self.interrupts
    }

    /// Get access to the local APIC
    pub fn apic(&self) -> &SpinLock<Option<LocalApic>, InterruptLock> {
        &self.apic
    }

    /// Set the current core's APIC ID
    ///
    /// This function can be only called once
    pub fn set_apic_id(&self, apic_id: u32) {
        self.apic_id.set(apic_id);
    }

    /// Get access to the current core's APIC ID if initialized
    pub fn apic_id(&self) -> Option<u32> {
        self.apic_id.initialized().then_some(*self.apic_id.get())
    }

    /// Get the preferred memory range for the currently running core.
    /// Returns `None` if there's no valid APIC ID or we have no knowledge of
    /// NUMA.
    pub fn mem_range<'a>(&self) -> Option<&'a rangeset::RangeSet> {
        crate::mm::mem_range()
    }

    /// Set that we're currently in an interrupt
    pub fn enter_interrupt(&self) -> AutoRefCountGuard {
        self.interrupt_depth.increment()
    }

    /// Get whether we're currently in an interrupt
    pub fn in_interrupt(&self) -> bool {
        self.interrupt_depth.count() != 0
    }

    /// Set that we're currently in an exception
    pub fn enter_exception(&self) -> AutoRefCountGuard {
        self.exception_depth.increment()
    }

    /// Get whether we're currently in an exception
    pub fn in_exception(&self) -> bool {
        self.exception_depth.count() != 0
    }

    /// Disable interrupts in a nesting manner.
    ///
    /// The "nesting manner" here means that if multiple `disable_interrupts()`
    /// are called, that many `enable_interrupts()` must be called before the
    /// interrupts are enabled again.
    #[track_caller]
    pub unsafe fn disable_interrupts(&self) {
        let x = self.interrupt_disable_requests.fetch_add(1, Ordering::SeqCst);
        x.checked_add(1).expect("Overflow on disable interrupts outstanding");
        unsafe { cpu::disable_interrupts(); }
    }

    /// Enable interrupts in a nesting manner.
    ///
    /// As many `enable_interrupts()` must be called as there have been
    /// `disable_interrupts()` called.
    #[track_caller]
    pub unsafe fn enable_interrupts(&self) {
        let x = self.interrupt_disable_requests.fetch_sub(1, Ordering::SeqCst);
        x.checked_sub(1).expect("Overflow on enable interrupts outstanding");

        // Since it's possible interrupts can be enabled when we enter an
        // interrupt, if we acquire a lock in an interrupt and release it, it
        // may attempt to re-enable interrupts.
        // Thus, we never allow enabling interrupts from an interrupt handler.
        // This means interrupts will correctly get re-enabled in this case when
        // the IRET loads the old interrupt flag.
        if !core!().in_interrupt() && x == 1 {
            unsafe { cpu::enable_interrupts(); }
        }
    }
}

/// Returns a reference to the data local to this core
#[inline]
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
pub fn init(shared: page_table::PhysAddr) {
    // Allocate an ID for this core
    let core_id = NEXT_CORE_ID.fetch_add(1, Ordering::SeqCst);

    // Offset the SHARED pointer into our physical window and get its reference
    let shared = crate::mm::phys_ptr(shared);
    let shared = unsafe { &*(shared.0 as *const Shared<DummyInterruptState>) };

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

    // Generate the freelists
    let free_lists = generate_freelists!(
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
    );

    // Create the struct
    let locals = CoreLocals {
        address: VirtAddr(core_locals_ptr),
        id: core_id,
        shared: unsafe {
            &*(shared as *const _ as *const Shared<InterruptLock>)
        },

        apic: SpinLock::new_no_preempt(None),
        apic_id: OnceLock::new(),

        interrupts: SpinLock::new_no_preempt(None),
        exception_depth: AutoRefCount::new(0),
        interrupt_depth: AutoRefCount::new(0),
        interrupt_disable_requests: AtomicUsize::new(0),

        free_lists,
    };

    unsafe {
        // Write the struct to the allocation
        core::ptr::write(core_locals_ptr as *mut CoreLocals, locals);

        // Set GS so we can access the locals from anywhere using `core!()`
        cpu::set_gs_base(core_locals_ptr as u64);
    }
}
