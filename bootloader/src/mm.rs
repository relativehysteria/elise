//! Memory manager for the bootloader
//!
//! This memory manager uses boot service allocation routines for as long as the
//! boot services are present. Once the boot services are exited, the memory map
//! is acquired and then used as the backing memory.

use core::alloc::{ GlobalAlloc, Layout };
use crate::SHARED;
use crate::efi::{system_table, Status};
use crate::efi::memory::MemoryType;

#[alloc_error_handler]
/// Handler for allocation errors, likely OOMs;
/// simply panic, notifying that we can't satisfy the allocation
fn alloc_error(_layout: Layout) -> ! {
    panic!("Allocation error")
}

#[global_allocator]
/// Global allocator for the bootloader; this uses boot services for as long as
/// it can. After they are exited, it uses physical memory* as a backlog and
/// __doesn't__ handle fragmentation. Only memory that won't have to be freed
/// between soft reboots should be allocated to prevent fragmentation.
///
/// * In reality, it doesn't quite use physical memory as its backlog. The
/// bootloader will be in long mode once it gets execution from the UEFI
/// firmware, which in turn requires paging. The UEFI standard defines the
/// paging structures to have identity map all the way through, so this
/// _technically_ (indirectly) uses physical memory just as one would expect.
static mut GLOBAL_ALLOCATOR: GlobalAllocator = GlobalAllocator {
    alloc_fn:   boot_alloc,
    dealloc_fn: boot_dealloc,
};

/// Dummy structure that implements the [`GlobalAlloc`] trait
struct GlobalAllocator {
    /// The allocation function implemented with either the boot services or
    /// through the memory map returned by UEFI.
    alloc_fn: unsafe fn(Layout) -> *mut u8,

    /// The deallocation function implemented with either the boot services or
    /// through the memory map returned by UEFI.
    dealloc_fn: unsafe fn(*mut u8, Layout),
}

unsafe impl GlobalAlloc for GlobalAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe { (self.alloc_fn)(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { (self.dealloc_fn)(ptr, layout); }
    }
}

/// Causes the bootloader allocator to stop using boot services for memory
/// allocation functions.
///
/// It is essential that the bootloader only ever has one core doing the
/// allocations and deallocations for them to be done safely and as such this
/// function is unsafe.
///
/// To prevent memory leaks, all free-able structures should be freed before
/// executing this call, because all boot service allocations are of the
/// `LoaderData` type and we decide not to touch that memory post boot svc exit.
pub unsafe fn switch_to_post_boot(memory_map: rangeset::RangeSet) {
    // Initialize the memory map
    let mut free_mem = SHARED.free_memory.lock();
    *free_mem = Some(memory_map);

    // Swap the allocation routines to stop using the boot services
    unsafe {
        GLOBAL_ALLOCATOR.alloc_fn = post_boot_alloc;
        GLOBAL_ALLOCATOR.dealloc_fn = post_boot_dealloc;
    }
}

/// Allocate memory using the boot service `allocate_pool()`
unsafe fn boot_alloc(layout: Layout) -> *mut u8 {
    unsafe {
        // Attempt to allocate the requesed memory
        let buffer: *mut *mut u8 = core::ptr::null_mut();
        let status = (system_table().boot_svc.allocate_pool)(
            MemoryType::LoaderData, layout.size(), buffer);

        // If the allocation fails for whatever reason, return a null pointer as
        // required by rust, otherwise return a pointer to the allocated memory
        if status != Status::Success {
            core::ptr::null_mut() as *mut u8
        } else {
            *buffer as *mut u8
        }
    }
}

/// Free memory using the boot service `free_pool()`
unsafe fn boot_dealloc(ptr: *mut u8, _layout: Layout) {
    unsafe {
        if (system_table().boot_svc.free_pool)(ptr) != Status::Success {
            panic!("Couldn't free a memory pool using the boot services");
        }
    }
}

/// Allocate memory using the memory map returned by UEFI
unsafe fn post_boot_alloc(layout: Layout) -> *mut u8 {
    // Get access to the "physical memory", allocate some bytes and return
    // the pointer
    let mut phys_mem = SHARED.free_memory.lock();
    phys_mem.as_mut().and_then(|x| {
        x.allocate(layout.size(), layout.align()).ok()?
    }).unwrap_or(0) as *mut u8
}

/// Free up memory using the memory map returned by UEFI
unsafe fn post_boot_dealloc(ptr: *mut u8, layout: Layout) {
    // Get access to the physical memory rangeset and try to insert a new
    // range into it. If the pointer was allocated by [`alloc()`], it should
    // be correct. Here's the classical `free()` safety message:
    // ---------------------------------------------
    // If the pointer was not allocated by [`alloc()`], it can 'free up'
    // 1) ranges that can't be satisfied by the backing physical memory
    // 2) ranges that don't belong to the caller
    let mut phys_mem = SHARED.free_memory.lock();
    let ptr = ptr as usize;
    phys_mem.as_mut().and_then(|x| {
        let end = ptr.checked_add(layout.size().checked_sub(1)?)?;
        x.insert(rangeset::Range::new(ptr, end).unwrap())
            .expect("Couldn't create a free memory range during dealloc");
        Some(())
    }).expect("Cannot free memory without initialized memory manager.");
}
