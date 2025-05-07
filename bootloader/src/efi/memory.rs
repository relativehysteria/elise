use crate::efi::{Status, SystemTablePtr, BootloaderImagePtr};
use rangeset::{RangeSet, Range};

/// Errors returned by memory routines
#[derive(Debug)]
pub enum Error {
    /// Memory map expected a larger array
    WrongMemoryMapSize(usize),

    /// Couldn't exit the boot services
    ExitBootSvcFailed,

    /// Some calculation overflowed while creating the free memory map
    MemoryMapOverflow,
}

/// Memory descriptors returned by the `get_memory_map()` boot service
#[derive(Debug, Copy, Clone)]
#[repr(C, align(16))]
pub struct MemoryDescriptor {
    /// Type of the memory region
    pub mem_type: MemoryType,

    /// Physical address of the first byte in the memory region
    pub phys_addr: usize,

    /// Virtual address of the first byte in the memory region
    pub virt_addr: usize,

    /// Number of 4 KiB pages in the memory region
    pub n_pages: u64,

    /// Attributes of the memory region that describe the bit mask of
    /// capabilities for that memory region.
    ///
    /// These attributes are not defined anywhere in the bootloader code but
    /// they can be found in related definitions of the `get_memory_map()` boot
    /// service, chapter 7.2
    attribute: u64,
}

impl MemoryDescriptor {
    /// Returns a memory descriptor whose byte map is filled with 0s.
    const fn empty() -> Self {
        MemoryDescriptor {
            mem_type: MemoryType::Reserved,
            phys_addr: 0,
            virt_addr: 0,
            n_pages: 0,
            attribute: 0,
        }
    }
}

/// Type of memory region
#[derive(Debug, Copy, Clone)]
#[repr(u32)]
pub enum MemoryType {
    /// Not usable
    Reserved = 0,

    /// Code portion of a loaded UEFI application
    LoaderCode,

    /// Data portion of a loaded UEFI application
    LoaderData,

    /// Code portion of a loaded boot service driver
    BootServicesCode,

    /// Data portion of a loaded boot service driver
    BootServicesData,

    /// Code portion of a loaded runtime driver
    RuntimeServicesCode,

    /// Data portion of a loaded runtime driver
    RuntimeServicesData,

    /// Free (unallocated) memory
    ConventionalMemory,

    /// Memory in which errors have been detected
    UnusableMemory,

    /// Memory holding ACPI tables
    ACPIReclaimMemory,

    /// Reserved for use by the firmware
    ACPIMemoryNVS,

    /// Used by system firmware to request that a memory-mapped IO region be
    /// mapped by the OS to a virtual address so it can be accessed by EFI
    /// runtime services
    MemoryMappedIO,

    /// System memory-mapped IO region that is used to translate memory cycles
    /// to IO cycles by the processor
    MemoryMappedIOPortSpace,

    /// Address space reserved by the firmware for code that is part of the
    /// processor
    PalCode,

    /// Like `ConventionalMemory`, but also supports byte-addressable
    /// non-volatility
    PersistentMemory,

    /// Memory type not supported by our system whatsoever
    Unsupported,
}


impl MemoryType {
    /// Returns whether this memory type is available for general use after
    /// `exit_boot_services()` has been called
    pub fn available_post_boot_svc_exit(&self) -> bool {
        match self {
            MemoryType::BootServicesCode   |
            MemoryType::BootServicesData   |
            MemoryType::PersistentMemory   |
            MemoryType::ConventionalMemory => true,
            ______________________________ => false,
        }
    }
}

impl From<u32> for MemoryType {
    fn from(val: u32) -> MemoryType {
        match val {
             0 => MemoryType::Reserved,
             1 => MemoryType::LoaderCode,
             2 => MemoryType::LoaderData,
             3 => MemoryType::BootServicesCode,
             4 => MemoryType::BootServicesData,
             5 => MemoryType::RuntimeServicesCode,
             6 => MemoryType::RuntimeServicesData,
             7 => MemoryType::ConventionalMemory,
             8 => MemoryType::UnusableMemory,
             9 => MemoryType::ACPIReclaimMemory,
            10 => MemoryType::ACPIMemoryNVS,
            11 => MemoryType::MemoryMappedIO,
            12 => MemoryType::MemoryMappedIOPortSpace,
            13 => MemoryType::PalCode,
            14 => MemoryType::PersistentMemory,
            _  => MemoryType::Unsupported,
        }
    }
}

/// This is the maximum amount of memory descriptors we expect to get from UEFI.
/// The larger your system memory, the more descriptors should be expected.
const N_MEM_DESC: usize = 2048;

/// Get a memory map of [`MemoryDescriptor`]s and exit the boot services
pub unsafe fn memory_map_exit(sys: SystemTablePtr, image: BootloaderImagePtr)
        -> Result<RangeSet, Error> {
    // Get the pointer to the services required
    let boot_svc = unsafe { (*sys.0).boot_svc };
    let get_memory_map = boot_svc.get_memory_map;
    let exit_boot_svc  = boot_svc.exit_boot_services;

    // Allocate a buffer for the memory map
    let mut memory_map = [MemoryDescriptor::empty(); N_MEM_DESC];

    // Initialize arguments for the `get_memory_map()` services
    let mut size: usize = core::mem::size_of_val(&memory_map);
    let mut map_key: usize = 0;
    let mut desc_size: usize = 0;
    let mut desc_version: u32 = 0;

    // Populate the memory map
    let ret = Status::from(unsafe {
        get_memory_map(&mut size, memory_map.as_mut_ptr(), &mut map_key,
                       &mut desc_size, &mut desc_version)
    });

    // Make sure we got the map correctly
    if ret != Status::Success { return Err(Error::WrongMemoryMapSize(size)); }

    // Transmute the array to an array of descriptors with the correct length
    let memory_map = unsafe {
        core::slice::from_raw_parts(
            memory_map.as_ptr(),
            size / core::mem::size_of::<MemoryDescriptor>())
    };

    // Exit the boot services
    let ret = Status::from(unsafe { exit_boot_svc(image, map_key) });

    // Make sure we have exited successfully
    if ret != Status::Success { return Err(Error::ExitBootSvcFailed) };

    // Now, only retain the memory that we are free to use in a memory allocator
    let mut free_memory = RangeSet::new();
    for desc in memory_map.iter() {
        // Skip all regions that will become invalid post boot services exit
        if !desc.mem_type.available_post_boot_svc_exit() { continue; }

        // Calculate the end of this memory
        let offset = (desc.n_pages as usize).checked_mul(4096)
            .ok_or(Error::MemoryMapOverflow)?;
        let end = desc.phys_addr.checked_add(offset - 1)
            .ok_or(Error::MemoryMapOverflow)?;

        // Write the memory down. This will only ever return errors if the UEFI
        // sabotages us and gives us corrupted information. At that point it is
        // safer to just panic instead of handling the errors.
        free_memory.insert(Range::new(desc.phys_addr as u64, end as u64)
            .unwrap()).unwrap();
    }

    // Reserve the first page to avoid writing into legacy structures
    free_memory.remove(Range::new(0x0000, 0xFFFF).unwrap()).unwrap();

    // Also reserve the legacy video/BIOS hole in case UEFI returns it as
    // conventional memory when it technically shouldn't be
    free_memory.remove(Range::new(0xA0000, 0xFFFFF).unwrap()).unwrap();

    // Return the memory
    Ok(free_memory)
}
