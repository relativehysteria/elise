//! Routines for creating and manipulating 4-level x86_64 page tables

use core::alloc::Layout;

/// Page table flag indicating this page or table is present
const PAGE_PRESENT: u64 = 1 << 0;

/// Page table flag indicating this page or table is writable
const PAGE_WRITE: u64 = 1 << 1;

/// Page table flag indicating this page or table is accessible by userspace
const PAGE_USER: u64 = 1 << 2;

/// Page table flag indicating this page entry is a large page
const PAGE_SIZE: u64 = 1 << 7;

/// Page table flag indicating this page or table is not executableu
const PAGE_NXE: u64 = 1 << 63;


#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Strongly typed physical address.
pub struct PhysAddr(pub u64);

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// A strongly typed virtual address.
pub struct VirtAddr(pub u64);

/// A trait that allows generic access to physical memory.
///
/// This allows handling of the physical to virtual translations that are done
/// during page table walks.
///
/// It can be implemented on paging setups as long as they are identity mapped
/// and translate correctly to physical memory.
pub trait PhysMem {
    /// Get a virtual address to memory which constains the raw physical memory
    /// at `paddr` for `size` bytes
    unsafe fn translate(&mut self, paddr: PhysAddr, size: usize)
        -> Option<*const u8>;

    /// Get a virtual address to memory which constains the raw physical memory
    /// at `paddr` for `size` bytes, with mutable access
    unsafe fn translate_mut(&mut self, paddr: PhysAddr, size: usize)
        -> Option<*mut u8>;

    /// Allocate physical memory with a requested `layout`
    fn alloc_phys(&mut self, layout: Layout) -> Option<PhysAddr>;

    /// Same as `alloc_phys()` but the memory will be zeroed out
    fn alloc_phys_zeroed(&mut self, layout: Layout) -> Option<PhysAddr> {
        // Allocate the memory
        let allocation = self.alloc_phys(layout)?;

        // Zero it out
        unsafe {
            let bytes = self.translate_mut(allocation, layout.size())?;
            core::ptr::write_bytes(bytes, 0, layout.size());
        }

        Some(allocation)
    }
}

/// A strongly typed structure for paging memory permission bits.
///
/// `read` isn't here because all present pages must be readable
pub struct Permission {
    /// Marks the memory as writable
    pub write: bool,

    /// Marks the memory as executable
    pub execute: bool,

    /// Marks the memory as accessible by the userspace
    pub user: bool,
}

impl Permission {
    /// Creates a new permission struct given arguments
    pub fn new(write: bool, execute: bool, user: bool) -> Self {
        Self { write, execute, user }
    }

    /// Returns the bit mask of this permission struct
    fn bits(&self) -> u64 {
        0 | if self.write   { PAGE_WRITE } else { 0 }
          | if self.user    { PAGE_USER  } else { 0 }
          | if self.execute { 0 } else { PAGE_NXE }
    }
}

#[repr(u64)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Different page sizes for 4-level x86_64 paging
pub enum PageType {
    Page4K = 4096,
    Page2M = 2 * 1024 * 1024,
    Page1G = 1 * 1024 * 1024 * 1024,
}

impl PageType {
    /// Returns the `PAGE_SIZE` bit if this page type is not 4-KiB
    fn size_bit(&self) -> u64 {
        if *self == PageType::Page4K { 0 } else { PAGE_SIZE }
    }
}

/// Request for a new page table mapping
pub struct MapRequest<F: Fn(u64) -> u8> {
    /// The Page type of the new entry to be mapped
    pub page_type: PageType,

    /// A new page table entry should be mapped in at this address
    pub vaddr: VirtAddr,

    /// The length of the new allocation in bytes
    pub size: u64,

    /// The permission bits for the new entry
    pub permissions: Permission,

    /// The function that will be called on each byte of the new page.
    /// It will be invoked with the current offset into the mapping and the
    /// return value will be used to initialize that byte.
    pub init: Option<F>,
}

impl<F: Fn(u64) -> u8> MapRequest<F> {
    /// Creates a new mapping request.
    ///
    /// The entry will be `PageType::Page4K` and the memory won't be initialized
    pub fn new(vaddr: VirtAddr, size: u64, permissions: Permission) -> Self {
        Self {
            page_type: PageType::Page4K,
            vaddr,
            size,
            permissions,
            init: None::<F>
        }
    }

    /// Sets the page type for the new page mapping
    pub fn page_type(mut self, ptype: PageType) -> Self {
        self.page_type = ptype;
        self
    }

    /// Sets the initialization function for the new memory mapping
    pub fn set_init(mut self, init_func: F) -> Self {
        self.init = Some(init_func);
        self
    }
}

#[repr(C)]
/// A 64-bit x86 page table
pub struct PageTable {
    table: PhysAddr,
}

impl PageTable {
    /// Create a new empty page table, allocating it in physical memory using
    /// `phys_mem`
    pub fn new<P: PhysMem>(phys_mem: &mut P) -> Option<Self> {
        // Allocate the root level table
        let table = phys_mem.alloc_phys_zeroed(
            Layout::from_size_align(4096, 4096).unwrap())?;

        Some(PageTable { table })
    }

    #[inline]
    /// Returns the address of the page table
    pub fn table(&self) -> PhysAddr {
        self.table
    }

    /// Create a 4-KiB page table entry within this page table
    pub fn map<F: Fn(u64) -> u8, P: PhysMem>(
            &mut self, phys_mem: &mut P, request: MapRequest<F>) -> Option<()> {
        let vaddr = request.vaddr.0;

        // Make sure the virtual address is aligned to the page size request
        if request.size <= 0 || (vaddr & (request.page_type as u64 - 1)) != 0 {
            return None;
        }

        // Compute the end virtual address of this mapping
        let end_vaddr = vaddr.checked_add(request.size - 1)?;

        // Get the page size for this mapping
        let page_size = request.page_type as u64 as usize;

        // Go through each page, allocate it and initialize the memory
        for vaddr in (vaddr..=end_vaddr).step_by(page_size) {
            // Allocate the page
            let page = phys_mem.alloc_phys(
                Layout::from_size_align(page_size, page_size).unwrap())?;

            // Create the page table entry
            let entry = page.0 | PAGE_PRESENT
                | request.permissions.bits()
                | request.page_type.size_bit();

            // If there is an initialization function, use it to initialize the
            // memory
            if let Some(init) = &request.init {
                // Create a slice from this page's physical memory
                let slice = unsafe {
                    let bytes = phys_mem.translate_mut(page, page_size)?;
                    core::slice::from_raw_parts_mut(bytes, page_size)
                };

                // Initialize all of the bytes in this allocation
                for (off, byte) in slice.iter_mut().enumerate() {
                    *byte = init(vaddr - request.vaddr.0 + off as u64);
                }
            }

            // Add this mapping to the page table
            // TODO: finish
        }
        None
    }
}
