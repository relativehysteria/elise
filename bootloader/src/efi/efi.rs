//! Generic EFI definitions

use crate::efi::memory::{MemoryDescriptor, MemoryType};

/// Handle to anything within the EFI spec
pub type Handle = *mut usize;

/// Handle to an EFI image
pub type ImageHandle = Handle;

/// The raw status value returned by EFI routines. This can be safely cast to
/// the [`Status`] value using `from()`
pub type RawStatus = isize;

/// A pointer to the system table that was passed to our bootloader by UEFI.
/// This struct exists only to take ownership of the pointer and to make it
/// impossible for other code to use when we don't want to
#[derive(Copy, Clone)]
#[repr(transparent)]
pub struct SystemTablePtr(pub *mut SystemTable);

/// A pointer to the bootloader image that was passed to us by UEFI.
/// This struct exists only to take ownership of the pointer and to make it
/// impossible for other code to use when we don't want to
#[repr(transparent)]
pub struct BootloaderImagePtr(pub ImageHandle);

/// UEFI defined global unique ID
#[derive(Copy, Clone, Debug, PartialEq)]
#[repr(C, packed)]
#[allow(missing_docs)]
pub struct Guid {
    pub d1: u32,
    pub d2: u16,
    pub d3: u16,
    pub d4: [u8; 8],
}

impl Guid {
    /// Returns a new Guid
    pub const fn new(d1: u32, d2: u16, d3: u16, d4: [u8; 8]) -> Self {
        Self { d1, d2, d3, d4 }
    }
}

/// Structure that precedes all of the standard EFI table types
#[derive(Debug, Copy, Clone, PartialEq)]
#[repr(C)]
pub struct TableHeader {
    /// Signature that identifies the type of table that follows
    pub signature: u64,

    /// The revision of the EFI specification to which this table conforms
    pub revision: u32,

    /// The size, in bytes, of the entire table including this header
    pub header_size: u32,

    /// The 32-bit CRC for the entire table.
    ///
    /// This value is computed by setting this field to 0, and computing the
    /// 32-bit CRC for `header_size` bytes
    pub crc32: u32,

    /// Reserved field that must be set to 0
    reserved: u32
}

/// Contains pointers to the runtime and boot services tables
#[allow(dead_code)]
#[repr(C)]
pub struct SystemTable {
    /// The table header for this table
    pub header: TableHeader,

    /// A pointer to a cstring that identifies the vendor that produces the
    /// system firmware for the platform
    pub fw_vendor: *const u16,

    /// A firmware vendor specific value that identifies the revision of the
    /// system firmware for the platform
    pub fw_revision: u32,

    // Following are pointers to structures that won't be used in the bootloader

    console_in_handle:     *const usize,
    con_in:                *const usize,
    console_out_handle:    *const usize,
    con_out:               *const usize,
    standard_error_handle: *const usize,
    std_err:               *const usize,
    runtime_svc:           *const usize,

    /// Pointer to the EFI boot services table
    pub boot_svc: &'static BootServices,

    /// The number of system configuration tables in the buffer `cfg_tables`
    pub n_cfg_entries: usize,

    /// A pointer to the system configuration tables
    pub cfg_tables: *const ConfigTable,
}

/// Contains a set of GUID/pointer pairs compromised of the `cfg_table` field in
/// the [`SystemTable`]
#[derive(Debug, Copy, Clone, PartialEq)]
#[repr(C)]
pub struct ConfigTable {
    /// GUID identifying the configuration table
    pub guid: Guid,

    /// Pointer to the table associated with this GUID
    pub table: *const usize,
}

/// Containes pointers to all of the boot services
#[derive(Debug, Copy, Clone, PartialEq)]
#[repr(C)]
pub struct BootServices {
    /// The table header for this struct
    pub header: TableHeader,

    // Following are pointers to unused functions

    raise_tpl:      *const usize,
    restore_tpl:    *const usize,
    allocate_pages: *const usize,
    free_pages:     *const usize,

    /// Returns the current boot services memory map and memory map key
    pub get_memory_map:
        unsafe fn(memory_map_size:    &mut usize,
                  memory_map:         *mut MemoryDescriptor,
                  map_key:            &mut usize,
                  descriptor_size:    &mut usize,
                  descriptor_version: &mut u32) -> RawStatus,


    /// Allocate pool memory
    pub allocate_pool:
        unsafe fn(pool_type: MemoryType,
                  size:      usize,
                  buffer:    *mut *mut u8) -> RawStatus,

    /// Return pool memory to the system
    pub free_pool: unsafe fn(buffer: *mut u8) -> RawStatus,

    // Following are pointers to unused functions

    create_event:                 *const usize,
    set_timer:                    *const usize,
    wait_for_event:               *const usize,
    signal_event:                 *const usize,
    close_event:                  *const usize,
    check_event:                  *const usize,
    install_protocol_interface:   *const usize,
    reinstall_protocol_interface: *const usize,
    uninstall_protocol_interface: *const usize,
    handle_protocol:              *const usize,
    reserved:                     *const usize,
    register_protocol_notify:     *const usize,
    locate_handle:                *const usize,
    locate_device_path:           *const usize,
    install_configuration_table:  *const usize,
    load_image:                   *const usize,
    start_image:                  *const usize,
    exit:                         *const usize,
    unload_image:                 *const usize,

    /// Terminates boot services
    pub exit_boot_services:
        unsafe fn(image: BootloaderImagePtr, map_key: usize) -> RawStatus,

    // Following are pointers to unused functions

    get_next_monotonic_count:               *const usize,
    stall:                                  *const usize,
    set_watchdog_timer:                     *const usize,
    connect_controller:                     *const usize,
    disconnect_controller:                  *const usize,
    open_protocol:                          *const usize,
    close_protocol:                         *const usize,
    open_protocol_information:              *const usize,
    protocols_per_handle:                   *const usize,
    locate_handle_buffer:                   *const usize,
    locate_protocol:                        *const usize,
    install_multiple_protocol_interfaces:   *const usize,
    uninstall_multiple_protocol_interfaces: *const usize,
    calculate_crc32:                        *const usize,
    copy_mem:                               *const usize,
    set_mem:                                *const usize,
    create_event_ex:                        *const usize,
}
