//! Routines for creating and manipulating 4-level x86_64 page tables

#![no_std]

// The code here adheres to the Intel spec. A large portion of it was taken from
// Brandon -- it's good and I wouldn't write it any different (apart from the
// parts which I have indeed rewritten :D).

use core::alloc::Layout;

/// Page table flag indicating this page or table is present
pub const PAGE_PRESENT: u64 = 1 << 0;

/// Page table flag indicating this page or table is writable
pub const PAGE_WRITE: u64 = 1 << 1;

/// Page table flag indicating this page or table is accessible by userspace
pub const PAGE_USER: u64 = 1 << 2;

/// Page table flag indicating that accesses to memory described by this page or
/// table should be uncached
pub const PAGE_CACHE_DISABLE: u64 = 1 << 4;

/// Page table flag indicating this page entry is a large page
pub const PAGE_SIZE: u64 = 1 << 7;

/// Page table flag indicating this page or table is not executableu
pub const PAGE_NXE: u64 = 1 << 63;


/// Strongly typed physical address.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct PhysAddr(pub u64);

impl PhysAddr {
    /// Returns whether the physical address is aligned to `page_type`
    pub fn is_aligned_to_page(&self, page_type: PageType) -> bool {
        self.is_aligned(page_type as u64)
    }

    /// Returns whether the physical address is aligned to `val`
    pub fn is_aligned(&self, val: u64) -> bool {
        (self.0 & (!(val - 1))) == self.0
    }
}

/// A strongly typed virtual address.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct VirtAddr(pub u64);

impl VirtAddr {
    /// Returns whether the virtual address is aligned to `page_type`
    pub fn is_aligned_to_page(&self, page_type: PageType) -> bool {
        self.is_aligned(page_type as u64)
    }

    /// Returns whether the virtual address is aligned to `val`
    pub fn is_aligned(&self, val: u64) -> bool {
        (self.0 & (!(val - 1))) == self.0
    }
}

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

        // Return it
        Some(allocation)
    }
}

/// Mapping errors
#[derive(Debug, Clone)]
pub enum Error {
    /// An invalid page was attempted to be mapped in
    InvalidPage,

    /// Tried to remap a page
    MappedAlready,

    /// Attempted to map a larger page over smaller pages
    SmallerPagesPresent,

    /// Attempted to get page table components of a non-canonical address
    AddressNotCanonical,

    /// Attempted to map in an unaligned address
    AddressUnaligned,
}

/// Paging memory access permissions.
///
/// This struct defines the access rights for a memory page.
///
/// Note: The `read` permission is not included because all present pages are
/// implicitly readable.
#[derive(Debug, Clone)]
pub struct Permissions {
    /// Allows write access to the memory page
    pub write: bool,

    /// Allows execution of instructions from the memory page
    pub execute: bool,

    /// Allows access to the memory page from user mode
    pub user: bool,

    /// Disables caching for the memory page
    pub cache: bool,
}

impl Permissions {
    /// Returns a new instance with the specified access rights.
    ///
    /// The page will be assumed to be cached by default.
    pub fn new(write: bool, execute: bool, user: bool) -> Self {
        Self { write, execute, user, cache: false }
    }

    /// Returns a new instance with the specified access rights,
    /// ensuring the page is uncached
    pub fn uncached(write: bool, execute: bool, user: bool) -> Self {
        Self { write, execute, user, cache: true }
    }

    /// Computes the corresponding bitmask for the current permission set.
    ///
    /// This bitmask can be used to configure hardware page tables.
    fn bits(&self) -> u64 {
        0 | if self.write   { PAGE_WRITE } else { 0 }
          | if self.user    { PAGE_USER  } else { 0 }
          | if self.execute { 0 } else { PAGE_NXE }
          | if self.cache   { 0 } else { PAGE_CACHE_DISABLE }
    }
}

/// Different page sizes for 4-level x86_64 paging
#[repr(u64)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
#[derive(Debug, Clone)]
pub struct MapRequest {
    /// The Page type of the new entry to be mapped
    pub page_type: PageType,

    /// A new page table entry should be mapped in at this address
    pub vaddr: VirtAddr,

    /// The length of the new allocation in bytes
    pub size: u64,

    /// The permission bits for the new entry
    pub permissions: Permissions,
}

impl MapRequest {
    /// Creates a new mapping request.
    ///
    /// Returns `None` if the `vaddr` is not aligned to the `page_type`
    pub fn new(vaddr: VirtAddr, page_type: PageType, size: u64,
               permissions: Permissions) -> Result<Self, Error> {
        vaddr.is_aligned_to_page(page_type).then_some(
            Self {
                page_type,
                vaddr,
                size,
                permissions,
            }
        ).ok_or(Error::AddressUnaligned)
    }
}

/// The paging components of a page table mapping.
#[derive(Debug, Clone, Copy, Default)]
pub struct Mapping {
    /// Physical address of the Page Map Level 4 entry
    pub pml4e: Option<PhysAddr>,

    /// Physical address of the Page Directory Pointer entry
    pub pdpe: Option<PhysAddr>,

    /// Physical address of the Page Directory entry
    pub pde: Option<PhysAddr>,

    /// Physical address of the Page Table entry
    pub pte: Option<PhysAddr>,

    /// Physical address of the base of the page, offset into the page, and the
    /// original raw page table entry
    pub page: Option<(PhysAddr, u64, u64)>,
}

impl Mapping {
    /// Returns the base virtual address of this page, if it exists
    pub fn virt_base(&self) -> Option<VirtAddr> {
        // Make sure this page is mapped in
        if self.page.is_none() { return None; }

        /// Size of a page table entry
        const ES: u64 = core::mem::size_of::<u64>() as u64;

        Some(VirtAddr(cpu::canonicalize_address(16,
            ((self.pml4e.unwrap_or(PhysAddr(0)).0 & 0xFFF) / ES) << 39 |
            ((self.pdpe .unwrap_or(PhysAddr(0)).0 & 0xFFF) / ES) << 30 |
            ((self.pde  .unwrap_or(PhysAddr(0)).0 & 0xFFF) / ES) << 21 |
            ((self.pte  .unwrap_or(PhysAddr(0)).0 & 0xFFF) / ES) << 12
        )))
    }

    /// Returns the size of this page
    pub fn page_type(&self) -> Option<PageType> {
        // Make sure this page is mapped in
        if self.page.is_none() { return None; }

        if self.pde.is_none() { return Some(PageType::Page1G); }
        if self.pte.is_none() { return Some(PageType::Page2M); }
        Some(PageType::Page4K)
    }

    /// Returns the components of `vaddr`
    fn get_indices(vaddr: VirtAddr) -> [u64; 4] {
        [
            (vaddr.0 >> 39) & 0x1FF,
            (vaddr.0 >> 30) & 0x1FF,
            (vaddr.0 >> 21) & 0x1FF,
            (vaddr.0 >> 12) & 0x1FF,
        ]
    }
}

/// A 64-bit x86 page table
#[derive(Debug, Clone, PartialEq)]
#[repr(transparent)]
pub struct PageTable {
    /// The physical address of the top-level page table. This is typically the
    /// value in `cr3`
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

    /// Returns a `PageTable` struct with the value of CR3 as the table address
    pub unsafe fn from_cr3() -> Self {
        let mut cr3 = PhysAddr(0);
        unsafe { core::arch::asm!("mov {}, cr3", out(reg) cr3.0) }
        Self { table: cr3 }
    }

    /// Translate a virtual address in this page table into its components.
    pub fn components<P: PhysMem>(&self, phys_mem: &mut P, vaddr: VirtAddr)
            -> Result<Mapping, Error> {
        unsafe {
            (*(self as *const Self as *mut Self))
                .components_inner(phys_mem, vaddr)
        }
    }

    /// Translate a virtual address in this page table into its components.
    ///
    /// This is the internal function and shouldn't be used unless necessary.
    /// Use `translate()` instead.
    pub unsafe fn components_inner<P: PhysMem>(&mut self, phys_mem: &mut P,
            vaddr: VirtAddr) -> Result<Mapping, Error> {
        // Start with an empty mapping
        let mut ret = Mapping::default();

        // Make sure the address is canonical
        if cpu::canonicalize_address(16, vaddr.0) != vaddr.0 {
            return Err(Error::AddressNotCanonical);
        }

        // Get the components of the address
        let indices = Mapping::get_indices(vaddr);

        // Get the address of the page table
        let mut table = self.table;

        for (depth, &index) in indices.iter().enumerate() {
            // Get the physical address of the page table entry
            let ptp = PhysAddr(table.0 + index * size_of::<u64>() as u64);

            // Fill in the address of the entry we are decoding
            match depth {
                0 => ret.pml4e = Some(ptp),
                1 => ret.pdpe  = Some(ptp),
                2 => ret.pde   = Some(ptp),
                3 => ret.pte   = Some(ptp),
                _ => unreachable!(),
            }

            // Get a virtual address for this entry
            let vad = unsafe {
                phys_mem.translate(ptp, size_of::<u64>()).unwrap()
            };
            let ent = unsafe { core::ptr::read(vad as *const u64) };

            // Check if this page is present
            if (ent & PAGE_PRESENT) == 0 {
                // Page is not present, break out and stop the translation
                break;
            }

            // Update the table to point to the next level
            table = PhysAddr(ent & 0xffffffffff000);

            // Check if this is the page mapping and not pointing to a table
            if depth == 3 || (ent & PAGE_SIZE) != 0 {
                // Page size bit is not valid (reserved as 0) for the PML4E,
                // return out the partially walked table
                if depth == 0 { break; }

                // Determine the mask for this page size
                let page_mask = match depth {
                    1 => PageType::Page1G as u64 - 1,
                    2 => PageType::Page2M as u64 - 1,
                    3 => PageType::Page4K as u64 - 1,
                    _ => unreachable!(),
                };

                // At this point, the page is valid, mask off all bits that
                // arent part of the address
                let page_paddr = table.0 & !page_mask;

                // Compute the offset in the page for the `vaddr`
                let page_off = vaddr.0 & page_mask;

                // Store the page and offset
                ret.page = Some((PhysAddr(page_paddr), page_off, ent));

                // Translation done
                break;
            }
        }

        Ok(ret)
    }

    /// Create a 4-KiB page table entry within this page table, initializing all
    /// memory to 0.
    pub fn map<P: PhysMem>(&mut self, phys_mem: &mut P, request: MapRequest)
            -> Option<()> {
        self.map_init(phys_mem, request, None::<fn(u64) -> u8>)
    }

    /// Create a 4-KiB page table entry within this page table.
    ///
    /// If `ini` is `Some`, it will be invoked with the current offset into the
    /// mapping, and the return value from the closure will be used to
    /// initialize that byte.
    pub fn map_init<F: Fn(u64) -> u8, P: PhysMem>(
        &mut self,
        phys_mem: &mut P,
        request: MapRequest,
        init: Option<F>
    ) -> Option<()> {
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
            if let Some(init) = &init {
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
            unsafe {
                if self.map_raw(phys_mem, VirtAddr(vaddr), request.page_type,
                                entry).is_err() {
                    // XXX: Failed to map; anything we did will be leaked
                    return None;
                }
            }
        }

        Some(())
    }

    /// Map a `vaddr` to a raw page table entry `raw`, using the page size
    /// specified by `page_type`
    pub unsafe fn map_raw<P: PhysMem>(
            &mut self, phys_mem: &mut P, vaddr: VirtAddr,
            page_type: PageType, raw: u64) -> Result<(), Error> {
        // Non present or large pages without page size bit set are invalid
        if (raw & PAGE_PRESENT) == 0 ||
                ((page_type != PageType::Page4K) && (raw & PAGE_SIZE == 0)) {
            return Err(Error::InvalidPage);
        }

        // Determine the state of the existing mapping
        let mapping = self.components(phys_mem, vaddr)?;

        // Don't re-map pages
        if mapping.page.is_some() { return Err(Error::MappedAlready); }

        // Get all of the current mapping tables
        let mut entries = [
            mapping.pml4e,
            mapping.pdpe,
            mapping.pde,
            mapping.pte,
        ];

        // Get the number of the entries based on the page type
        let depth = match page_type {
            PageType::Page4K => 4,
            PageType::Page2M => 3,
            PageType::Page1G => 2,
        };

        // Don't map a large page over a table containing smaller pages
        if entries.get(depth).map_or(false, |x| x.is_some()) {
            return Err(Error::SmallerPagesPresent);
        }

        // After this point, the mapping _must_ be done
        assert!(mapping.pml4e.is_some());

        // Get the components of the address
        let indices = Mapping::get_indices(vaddr);

        // Create page tables as needed while walking to the final page
        for idx in 1..depth {
            // If there is a table mapped in, job is done
            if entries[idx].is_some() { continue; }

            // Allocate a new empty table
            let table = phys_mem.alloc_phys_zeroed(
                Layout::from_size_align(4096, 4096).unwrap()).unwrap();

            // Convert the address of the page table entry where we need
            // to insert the new table
            let ptr = unsafe {
                phys_mem.translate_mut(entries[idx - 1].unwrap(),
                                       core::mem::size_of::<u64>()).unwrap()
            };

            if idx >= 2 {
                // Get access to the entry with the reference count of the
                // table we're updating
                let ptr = unsafe {
                    phys_mem.translate_mut(entries[idx - 2].unwrap(),
                                           core::mem::size_of::<u64>())
                        .unwrap()
                };

                // Read the entry
                let nent = unsafe { core::ptr::read(ptr as *const u64) };

                // Update the reference count
                let in_use = (nent >> 52) & 0x3ff;
                let nent = (nent & !(0x3FF << 52)) | ((in_use + 1) << 52);

                // Write in the new entry
                unsafe { core::ptr::write(ptr as *mut u64, nent); }
            }

            // Insert the new table at the entry in the table above us
            unsafe {
                core::ptr::write(ptr as *mut u64,
                    table.0 | PAGE_USER | PAGE_WRITE | PAGE_PRESENT);
            }

            // Update the mapping state as we have changed the tables
            entries[idx] = Some(PhysAddr(
                table.0 + indices[idx] * core::mem::size_of::<u64>() as u64
            ));
        }

        {
            // Get access to the entry with the reference count of the
            // table we're updating with the new page
            let ptr = unsafe {
                phys_mem.translate_mut(entries[depth - 2].unwrap(),
                                       core::mem::size_of::<u64>())
                    .unwrap()
            };

            // Read the entry
            let nent = unsafe { core::ptr::read(ptr as *const u64) };

            // Update the reference count
            let in_use = (nent >> 52) & 0x3ff;
            let nent = (nent & !(0x3FF << 52)) | ((in_use + 1) << 52);

            // Write in the new entry
            unsafe { core::ptr::write(ptr as *mut u64, nent); }
        }

        // At this point, the tables have been created, and the page doesn't
        // already exist. Thus, we can write in the mapping!
        unsafe {
            let ptr = phys_mem.translate_mut(entries[depth - 1].unwrap(),
                core::mem::size_of::<u64>()).unwrap();
            core::ptr::write(ptr as *mut u64, raw);
        }

        Ok(())
    }
}
