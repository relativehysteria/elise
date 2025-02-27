//! Generic EFI definitions

use core::sync::atomic::{AtomicPtr, Ordering};
use crate::efi::Status;
use crate::efi::memory::{MemoryDescriptor, MemoryType};

/// Handle to anything within the EFI spec
pub type Handle = *const usize;

/// Handle to an EFI image
pub type ImageHandle = Handle;

/// The static pointer to the `SystemTable` structure that is passed to our
/// bootloader by UEFI on initialization
pub static SYSTEM_TABLE: AtomicPtr<SystemTable> =
    AtomicPtr::new(core::ptr::null_mut());

/// Returns the pointer to the ['SystemTable'] structure passed by UEFI to the
/// bootloader
pub fn system_table() -> &'static SystemTable {
    let ptr = SYSTEM_TABLE.load(Ordering::Relaxed);
    assert!(!ptr.is_null(), "System table not initialized");
    unsafe { &*ptr }
}

#[derive(Copy, Clone, Debug, PartialEq)]
#[repr(C, packed)]
#[allow(missing_docs)]
/// UEFI defined global unique ID
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

#[derive(Debug, Copy, Clone, PartialEq)]
#[repr(C)]
/// Structure that precedes all of the standard EFI table types
pub struct TableHeader {
    /// Signature that identifies the type of table that follows
    pub signature: u64,

    /// The revision of the EFI specificatio nto which this table conforms
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

#[allow(dead_code)]
#[repr(C)]
/// Contains pointers to the runtime and boot services tables
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

#[derive(Debug, Copy, Clone, PartialEq)]
#[repr(C)]
/// Contains a set of GUID/pointer pairs compromised of the `cfg_table` field in
/// the [`SystemTable`]
pub struct ConfigTable {
    /// GUID identifying the configuration table
    pub guid: Guid,

    /// Pointer to the table associated with this GUID
    pub table: *const usize,
}

#[derive(Debug, Copy, Clone, PartialEq)]
#[repr(C)]
/// Containes pointers to all of the boot services
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
                  descriptor_version: &mut u32) -> Status,


    /// Allocate pool memory
    pub allocate_pool:
        unsafe fn(pool_type: MemoryType,
                  size:      usize,
                  buffer:    *mut *mut u8) -> Status,

    /// Return pool memory to the system
    pub free_pool: unsafe fn(buffer: *mut u8) -> Status,

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
        unsafe fn(image_handle: Handle, map_key: usize) -> Status,

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
