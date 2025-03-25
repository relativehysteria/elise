//! Kernel memory allocation routines and structures

use core::sync::atomic::{AtomicU64, Ordering};
use core::alloc::{GlobalAlloc, Layout};
use page_table::{PhysMem, PhysAddr, VirtAddr};
use shared_data::{
    KERNEL_PHYS_WINDOW_BASE, KERNEL_PHYS_WINDOW_SIZE, KERNEL_VMEM_BASE};

#[repr(transparent)]
/// Wrapper around a rangeset that implemente the `PhysMem` trait
///
/// The bootloader should have created a physical window in our memory map for
/// our memory allocation needs
pub struct PhysicalMemory;

impl PhysMem for PhysicalMemory {
    unsafe fn translate(&mut self, paddr: PhysAddr, size: usize)
            -> Option<*const u8> {
        unsafe { self.translate_mut(paddr, size).map(|x| x as *const u8) }
    }

    unsafe fn translate_mut(&mut self, paddr: PhysAddr, size: usize)
            -> Option<*mut u8> {
        // Compute the ending physical address and make sure we don't overflow
        let end = (size as u64).checked_sub(1)?.checked_add(paddr.0)?;

        // Make sure this physical address fits inside our physical window
        if end >= KERNEL_PHYS_WINDOW_SIZE { return None; }

        // Convert the physical address into a linear address
        Some((paddr.0 + KERNEL_PHYS_WINDOW_BASE) as *mut u8)
    }

    fn alloc_phys(&mut self, layout: Layout) -> Option<PhysAddr> {
        // TODO:
        // * per-core free lists
        // * NUMA

        // Get access to physical memory
        let mut phys_mem = core!().shared.free_memory().lock();
        let phys_mem = phys_mem.as_mut()?;

        // Allocate directly from physical memory
        let allocation = phys_mem
            .allocate(layout.size() as u64, layout.align() as u64).ok()??;
        Some(PhysAddr(allocation))
    }
}

/// Find a free region of virtual memory that can hold `size` bytes and return
/// its virtual address.
///
/// This only 'allocates' a virtual address that is guaranteed to be unique
/// but does not map in the memory it points to. As such, this can be used to
/// get a virtual address for page mappings where the virtual address of the
/// mapping doesn't matter.
pub fn receive_vaddr_4k(size: u64) -> VirtAddr {
    /// Base address for virtual allocations
    static NEXT_VADDR: AtomicU64 = AtomicU64::new(KERNEL_VMEM_BASE);

    /// Gap between virtual allocations
    const GUARD_PAGE: u64 = shared_data::REGION_PADDING;

    // Make sure this is a 4k page aligned allocation
    assert!(size > 0 && (size & 0xfff) == 0,
        "Invalid size for virtual region allocation");

    // Get a new virtual region that is free
    let reserve = GUARD_PAGE.checked_add(size)
        .expect("Virtual address allocation overflow");
    let ret = NEXT_VADDR.fetch_add(reserve, Ordering::SeqCst);

    // If we can't add the reserved size to the return value, then the virtual
    // memory wrapped the 64-bit boundary
    ret.checked_add(reserve).expect("Virtual address range overflow");
    VirtAddr(ret)
}

#[alloc_error_handler]
/// Handler for allocation errors, likely OOMs;
/// simply panic, notifying that we can't satisfy the allocation
fn alloc_error(_layout: Layout) -> ! {
    panic!("ALlocation error!");
}

// TODO: document when NUMA and per-core free lists are implemented
#[global_allocator]
/// Global allocator for the kernel
pub static GLOBAL_ALLOCATOR: GlobalAllocator = GlobalAllocator;

#[derive(Debug)]
/// A structure that implements `GlobalAlloc` such that we can use it as the
/// global allocator
pub struct GlobalAllocator;

unsafe impl GlobalAlloc for GlobalAllocator {
    // TODO
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 { panic!(); }

    // TODO
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) { panic!(); }
}
