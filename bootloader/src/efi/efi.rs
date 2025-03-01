//! Generic EFI definitions

use core::sync::atomic::{AtomicPtr, Ordering};
use crate::efi::memory::{MemoryDescriptor, MemoryType};

/// Handle to anything within the EFI spec
pub type Handle = *mut usize;

/// Handle to an EFI image
pub type ImageHandle = Handle;

/// Revision of EFI protocol
pub type Revision = u64;

/// The raw status value returned by EFI routines. This can be safely cast to
/// the [`Status`] value using `from()`
pub type RawStatus = usize;

/// The static pointer to the `SystemTable` structure that is passed to our
/// bootloader by UEFI on initialization
static SYSTEM_TABLE: AtomicPtr<SystemTable> =
    AtomicPtr::new(core::ptr::null_mut());

/// The static pointer to the bootloader `ImageHandle` that is passed to our
/// bootloader by UEFI on initialization
static BOOTLOADER_IMAGE: AtomicPtr<usize> =
    AtomicPtr::new(core::ptr::null_mut());

/// Initialize the structures required for a function EFI interface for the
/// bootloader
pub fn init_efi(bootloader_image: *mut usize,
                system_table: *mut SystemTable) {
    SYSTEM_TABLE.store(system_table, Ordering::SeqCst);
    BOOTLOADER_IMAGE.store(bootloader_image, Ordering::SeqCst);
}

/// Returns a reference to the ['SystemTable'] structure passed by UEFI to the
/// bootloader
pub fn system_table() -> &'static SystemTable {
    let ptr = SYSTEM_TABLE.load(Ordering::Relaxed);
    assert!(!ptr.is_null(), "System table not initialized");
    unsafe { &*ptr }
}

/// Returns a reference to the bootloader's `ImageHandle` passed by UEFI to the
/// bootloader
pub fn bootloader_image() -> ImageHandle {
    let ptr = BOOTLOADER_IMAGE.load(Ordering::Relaxed);
    assert!(!ptr.is_null(), "Bootloader image not initialized");
    ptr
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

    /// Returns an array of handles that support a specified protocol
    ///
    /// The function returns an array of handles that match the `search_type`
    /// request. If the input value of `buffer_size` is too small, the function
    /// updates the `buffer_size` to the size of the buffer needed to obtain the
    /// array.
    pub locate_handle:
        unsafe fn(search_type: SearchType,
                  protocol:    &Guid,
                  search_key:  *const u8,
                  buffer_size: &mut usize,
                  buffer:      *mut Handle) -> RawStatus,

    // Following are pointers to unused functions

    locate_device_path:           *const usize,
    install_configuration_table:  *const usize,
    load_image:                   *const usize,
    start_image:                  *const usize,
    exit:                         *const usize,
    unload_image:                 *const usize,

    /// Terminates boot services
    pub exit_boot_services:
        unsafe fn(image_handle: Handle, map_key: usize) -> RawStatus,

    // Following are pointers to unused functions

    get_next_monotonic_count:               *const usize,
    stall:                                  *const usize,
    set_watchdog_timer:                     *const usize,
    connect_controller:                     *const usize,
    disconnect_controller:                  *const usize,

    /// Queries a handle to determine if it supports a specified protocol.
    /// If the protocol is supported by the handle, it opens the protocol on
    /// behalf of the calling agent.
    ///
    /// This function can return a wide variety of errors. Check the UEFI
    /// specification, chapter 7.3 to see which and why.
    ///
    /// Handles no longer in use must be freed using the `close_protocol()` boot
    /// service.
    pub open_protocol:
        unsafe fn(handle: Handle,
                  protocol: &Guid,
                  interface: *mut *mut u8,
                  agent: Handle,
                  controller: Handle,
                  attributes: u32) -> RawStatus,

    /// Closes a protocol on a handle that was opened using the
    /// `open_protocol()` boot service.
    pub close_protocol:
        unsafe fn(handle: Handle,
                  protocol: &Guid,
                  agent: Handle,
                  controller: Handle) -> RawStatus,

    // Following are pointers to unused functions

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

#[derive(Debug, Copy, Clone, PartialEq)]
#[repr(C)]
/// Specifies which handle(s) are to be returned by handle searching functions
pub enum SearchType {
    /// `protocol` and `search_key` are ignored and functions return an array of
    /// every handle in the system
    AllHandles,

    /// `search_key` supplies the `registration` value returned by the
    /// `register_protocol_notify()` service. The search function returns the
    /// next handle that is new for registration. Only one handle is returned at
    /// a time, starting with the first, and the caller must loop until no more
    /// handles are returned. `protocol` is ignored for this search type
    ByRegisterNotify,

    /// All handles that support `protocol` are returned. `search_key` is
    /// ignored for this search type
    ByProtocol
}
