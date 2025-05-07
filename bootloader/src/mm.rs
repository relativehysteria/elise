//! Physical memory manager for the bootloader

use core::alloc::{ GlobalAlloc, Layout };
use rangeset::{ RangeSet, Range };
use page_table::{PhysAddr, PhysMem};
use crate::SHARED;

/// Wrapper around a rangeset that implements the `PhysMem` trait.
///
/// Required for manipulating page tables in the bootloader
#[repr(transparent)]
pub struct PhysicalMemory<'a>(pub &'a mut RangeSet);

impl<'a> PhysMem for PhysicalMemory<'a> {
    unsafe fn translate(&mut self, paddr: PhysAddr, size: usize)
            -> Option<*const u8> {
        unsafe { self.translate_mut(paddr, size).map(|x| x as *const u8) }
    }

    unsafe fn translate_mut(&mut self, paddr: PhysAddr, size: usize)
            -> Option<*mut u8> {
        // Make sure we're not translating zero sized memory
        assert!(size > 0, "Attempted to translate zero size memory");

        // Convert the physical address into a `usize` which is addressable in
        // the bootloader
        let paddr: usize = paddr.0.try_into().ok()?;
        let _pend: usize = paddr.checked_add(size - 1)?;

        Some(paddr as *mut u8)
    }

    fn alloc_phys(&mut self, layout: Layout) -> Option<PhysAddr> {
        Some(PhysAddr(
            self.0.allocate(layout.size() as u64, layout.align() as u64)
                .expect("Failed to allocate physical memory")? as u64
        ))
    }
}

/// Initialize the global memory allocator using `memory` as the physical memory
/// backlog.
pub fn init(memory: RangeSet) {
    // If the memory has been already initialized, don't reinitialize it
    if SHARED.free_memory().lock().is_some() { return; }

    // Initialize the memory
    let mut free_mem = SHARED.free_memory().lock();
    *free_mem = Some(memory);
}

/// Handler for allocation error, likely OOMs;
/// simply panic, notifying that we can't satisfy the allocation.
#[alloc_error_handler]
fn alloc_error(_layout: Layout) -> ! {
    panic!("Allocation error!");
}

/// Global allocator for the bootloader; this just uses physical memory as a
/// backlog and __doesn't__ handle fragmentation. Only memory that won't have to
/// be freed between soft reboots should be allocated to prevent fragmentation.
#[global_allocator]
static GLOBAL_ALLOCATOR: GlobalAllocator = GlobalAllocator;

/// Dummy structure that implements the [`GlobalAlloc`] trait
struct GlobalAllocator;

unsafe impl GlobalAlloc for GlobalAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // Get access to the physical memory, allocate some bytes and return
        // the pointer
        let mut phys_mem = SHARED.free_memory().lock();
        phys_mem.as_mut().and_then(|x| {
            x.allocate(layout.size() as u64, layout.align() as u64).ok()?
        }).unwrap_or(0) as *mut u8
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // Get access to the physical memory rangeset and try to insert a new
        // range into it. If the pointer was allocated by [`alloc()`], it should
        // be correct. Here's the classical `free()` safety message:
        // ---------------------------------------------
        // If the pointer was not allocated by [`alloc()`], it can 'free up'
        // 1) ranges that can't be satisfied by the backing physical memory
        // 2) ranges that don't belong to the caller
        let mut phys_mem = SHARED.free_memory().lock();
        let ptr = ptr as usize;
        phys_mem.as_mut().and_then(|x| {
            let end = ptr.checked_add(layout.size().checked_sub(1)?)?;
            x.insert(Range::new(ptr as u64, end as u64).unwrap())
                .expect("Couldn't create a free memory range during dealloc");
            Some(())
        }).expect("Cannot free memory without initialized memory manager.");
    }
}
