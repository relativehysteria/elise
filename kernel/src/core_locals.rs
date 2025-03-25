//! A core-exclusive data structure which can be accessed via the `core!()`
//! macro.

use page_table::VirtAddr;
use shared_data::{Shared, KERNEL_SHARED_BASE};

#[allow(dead_code)]
#[derive(Clone)]
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

    // Create the struct
    let locals = CoreLocals {
        address: VirtAddr(core_locals_ptr),
        id:      core_id,
        shared,
    };

    unsafe {
        // Write the struct to the allocation
        core::ptr::write(core_locals_ptr as *mut CoreLocals, locals);

        // Set GS so we can access the locals from anywhere using `core!()`
        cpu::set_gs_base(core_locals_ptr as u64);
    }
}
