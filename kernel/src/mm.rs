//! Kernel memory allocation routines and structures

use alloc::vec::Vec;
use core::mem::size_of;
use core::sync::atomic::{AtomicU64, Ordering};
use core::alloc::{GlobalAlloc, Layout};

use oncelock::OnceLock;
use page_table::{
    PhysMem, PhysAddr, VirtAddr, MapRequest, Permissions, PageType};
use shared_data::{
    KERNEL_PHYS_WINDOW_BASE, KERNEL_PHYS_WINDOW_SIZE, KERNEL_VMEM_BASE};
use rangeset::RangeSet;

use crate::apic::{ApicDomains, MemoryDomains, MAX_APIC_ID};

/// Mappings of APIC IDs to their NUMA node memory ranges
///
/// Each index in the array corresponds to a logical core (by APIC ID), and the
/// associated `Option<RangeSet>` contains the memory ranges (if any) assigned
/// to the NUMA node associated with that APIC ID. If a particular core has no
/// associated memory range, the entry will be `None`.
static APIC_TO_MEM_RANGE: OnceLock<&[Option<RangeSet>]> = OnceLock::new();

/// Get the preferred memory range for the currently running APIC.
/// Returns none if there's no valid APIC ID or we have no knowledge of NUMA.
pub fn mem_range<'a>() -> Option<&'a RangeSet> {
    // If the mapping wasn't initialized yet, bail out
    if !APIC_TO_MEM_RANGE.initialized() { return None; }

    // Get the mapping
    let mapping = APIC_TO_MEM_RANGE.get();

    // Look up and return the preferred memory range
    unsafe { core!().apic_id().and_then(|x| mapping[x as usize].as_ref()) }
}

/// Registers NUMA mappings with the allocator. From this call on, all
/// allocations will be NUMA aware.
pub unsafe fn register_numa(ad: ApicDomains, mut md: MemoryDomains) {
    // Make sure we're not registering numa with bogus data
    let max_apic_id = *MAX_APIC_ID.get();
    assert!(max_apic_id != 0, "Registering NUMA before parsing ACPI");

    // Allocate the database
    let mut mappings = (0..=max_apic_id)
        .map(|_| None)
        .collect::<Vec<Option<RangeSet>>>();

    // Go through each APIC to domain mapping and store it in the database
    ad.iter().for_each(|(&apic, domain)| {
        mappings[apic as usize] = md.remove(domain)
    });

    // Store the apic mapping database as global!
    APIC_TO_MEM_RANGE.set(mappings.leak());
}

#[track_caller]
/// Offset a physical address into our physical window
pub fn phys_ptr(addr: PhysAddr) -> VirtAddr {
    VirtAddr(addr.0.checked_add(KERNEL_PHYS_WINDOW_BASE)
        .expect("Overflow when offsetting into physical window"))
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

#[inline]
/// Get mutable access to a slice of physical memory
pub fn slice_phys_mut<'a>(paddr: PhysAddr, size: u64) -> &'a mut [u8] {
    // Make sure the address doesn't overflow
    let end = size.checked_sub(1).and_then(|x| {
        x.checked_add(paddr.0)
    }).expect("Integer overflow");

    // Make sure we fit in our physical window
    assert!(end < KERNEL_PHYS_WINDOW_SIZE,
        "Physical address outside physical window");

    // Return out the slice
    unsafe {
        core::slice::from_raw_parts_mut(
            (KERNEL_PHYS_WINDOW_BASE + paddr.0) as *mut u8, size as usize)
    }
}


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
        // If someone wants to allocate a 4-KiB page from physical memory,
        // use our free lists
        if layout.size() == 4096 && layout.align() == 4096 {
            unsafe {
                let ptr = core!().free_list(layout).lock().pop();
                Some(PhysAddr(ptr as u64 - KERNEL_PHYS_WINDOW_BASE))
            }
        } else {
            // Get access to physical memory
            let mut phys_mem = core!().shared.free_memory().lock();
            let phys_mem = phys_mem.as_mut()?;

            // Allocate directly from physical memory
            let size = layout.size() as u64;
            let align = layout.align() as u64;
            let allocation = phys_mem
                .allocate_prefer(size, align, mem_range())
                .ok().flatten()?;
            Some(PhysAddr(allocation))
        }
    }
}

/// Freed allocation metadata
pub struct FreeListNode {
    /// Virtual address of the next node in the freelist.
    ///
    /// If it's `0`, it indicates that this is the last node in the freelist.
    next: VirtAddr,

    /// Number of free slots in the `free_addrs` array.
    ///
    /// This value tracks how many addresses are currently available for reuse
    /// within this node. Each slot corresponds to a previously freed memory
    /// block.
    ///
    /// This is basically the value that would be returned by `free_addrs.len()`
    /// if it wasn't dynamic.
    free_slots: usize,

    /// Virtual addresses of freed memory blocks.
    ///
    /// The array size is dynamic and can hold a variable number of free
    /// addresses, depending on the size of the allocation managed by the
    /// freelist. `free_slots` is the number of elements in this array.
    free_addrs: [*mut u8; 0],
}

// This specific freelist implementation was originally designed by Brandon
/// A freelist allocator that manages fixed-size memory blocks.
pub struct FreeList {
    /// Pointer to the first entry in the freelist
    head: VirtAddr,

    /// Size of allocations (in bytes) for this freelist
    size: usize,
}

impl FreeList {
    /// Create a new, empty freelist containings addresses to `size` allocations
    pub fn new(size: usize) -> Self {
        assert!(size.count_ones() == 1,
            "Freelist size not power of two");
        assert!(size >= size_of::<usize>(),
            "Freelist size must be at least pointer width");
        Self { head: VirtAddr(0), size }
    }

    #[inline]
    /// If the blocks backed by this freelist fit into a 4-KiB page,
    /// allocate the page from our _physical memory_ to back the blocks.
    ///
    /// This doesn't create any new virtual mapping and so it's infinitely
    /// better for TLBs and caches.
    ///
    /// Panics if the blocks backed by this freelist don't fit into a page.
    fn allocate_page_for_blocks(&mut self) {
        // Make sure we're not allocating for a block that can't be backed by it
        assert!(self.size <= 4096,
            "Can't allocate page for a freelist whose blocks don't fit in");

        // Allocate the page from physical memory
        let allocation = {
            let mut phys_mem = core!().shared.free_memory().lock();
            let phys_mem = phys_mem.as_mut().unwrap();

            phys_mem.allocate_prefer(4096, 4096, mem_range())
                .ok().flatten()
                .expect("Failed to allocate physical memory") as u64
        };
        // Split up this allocation into blocks backed by this freelist
        // and make them available
        for offset in (0..4096).step_by(self.size) {
            let vaddr = slice_phys_mut(
                PhysAddr(allocation + offset), self.size as u64).as_mut_ptr();
            unsafe { self.push(vaddr); }
        }
    }

    #[inline]
    /// Allocate a new virtual mapping for a block size backed by this freelist
    /// and return the mapping
    fn allocate_virt_block(&mut self) -> VirtAddr {
        // Get a virtual address for this allocation
        let vaddr = receive_vaddr_4k(self.size as u64);

        // Create the allocation request
        let request = MapRequest::new(
            vaddr, PageType::Page4K, self.size as u64,
            Permissions::new(true, false, false)).unwrap();

        // Acquire access to physical and virtual memory and map it in
        let mut pmem = PhysicalMemory;
        let mut table = core!().shared.kernel_pt().lock();
        let table = table.as_mut().unwrap();
        table.map(&mut pmem, request).expect("Failed to map memory");

        // Return the virutal address of this allocation
        vaddr
    }

    /// Get an address from this freelist
    pub unsafe fn pop(&mut self) -> *mut u8 {
        // If this freelist is empty, allocate memory to back up the allocations
        if self.head.0 == 0 {
            // If the blocks backed by this freelist are smaller than a page,
            // just point the blocks to our physical memory window.
            if self.size <= 4096 {
                self.allocate_page_for_blocks();
            // Blocks backed by this freelist don't fit into a page. Just
            // allocate new virtual memory for the block and return the pointer
            } else {
                return self.allocate_virt_block().0 as *mut u8;
            }
        }

        // At this point the freelist can allocate at least one block

        // For allocations that can't hold our stack-based freelist metadata,
        // use a linked list.
        if self.size <= size_of::<FreeListNode>() {
            // Save the current head and set it to the next node
            let allocation = self.head.0 as *mut FreeListNode;
            self.head = unsafe { (*allocation).next };
            return allocation as *mut u8
        }

        // Use the free list stack
        let list = unsafe { &mut *(self.head.0 as *mut FreeListNode) };

        // Calculate the available slots considering metadata overhead
        let available_slots = (self.size / size_of::<usize>()) -
            (size_of::<FreeListNode>() / size_of::<usize>());

        // If there's a free entry in the stack, return it
        if list.free_slots < available_slots {
            // Just grab the free entry
            let allocation = unsafe {
                *list.free_addrs.as_mut_ptr()
                    .add(list.free_slots)
            };

            // Update the number of free slots
            list.free_slots += 1;

            return allocation;
        }

        // If no free slots are available, use the head of the list as the
        // allocation
        let allocation = self.head;

        // Update the head to point to the next entry
        self.head = list.next;

        allocation.0 as *mut u8
    }

    /// Put an allocation back onto the free list
    pub unsafe fn push(&mut self, vaddr: *mut u8) {
        // For allocations that can't hold our stack-based freelist metadata,
        // use a linked list.
        if self.size <= size_of::<FreeListNode>() {
            // Write the old head into the newly freed address
            let vaddr = vaddr as *mut FreeListNode;
            unsafe { (*vaddr).next = self.head; }

            // Update the head
            self.head = VirtAddr(vaddr as u64);
            return;
        }

        // Check if there is room for this allocation in the free stack,
        // or if we need to create a new stack
        let check = self.head.0 == 0 ||
            unsafe { (*(self.head.0 as *const FreeListNode)).free_slots == 0 };
        if check {
            // No free slots, create a new stack out of the freed vaddr
            let list = unsafe { &mut *(vaddr as *mut FreeListNode) };

            // Calculate the available slots considering metadata overhead
            let available_slots = (self.size / size_of::<usize>()) -
                (size_of::<FreeListNode>() / size_of::<usize>());

            // Set the number of free slots to the maximum size, as all
            // entries are free in the stack
            list.free_slots = available_slots;

            // Update the next to point to the old head
            list.next = self.head;

            // Establish this as the new free list head
            self.head = VirtAddr(vaddr as *mut FreeListNode as u64);
            return;
        }

        // There's room in the current stack, just throw us in there
        let list = unsafe { &mut *(self.head.0 as *mut FreeListNode) };

        // Decrement the number of free slots
        list.free_slots -= 1;

        // Store our newly freed virtual address into this slot
        unsafe {
            *list.free_addrs.as_mut_ptr()
                .add(list.free_slots) = vaddr;
        }
    }
}

#[alloc_error_handler]
/// Handler for allocation errors, likely OOMs;
/// simply panic, notifying that we can't satisfy the allocation
fn alloc_error(_layout: Layout) -> ! {
    panic!("Allocation error!");
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
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // Allocate memory from our freelists
        unsafe { core!().free_list(layout).lock().pop() }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // Put the allocation back into our freelists
        unsafe { core!().free_list(layout).lock().push(ptr); }
    }
}
