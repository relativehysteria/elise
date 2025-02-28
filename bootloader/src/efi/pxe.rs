//! PXE routines and structures that utilize the EFI boot services as its
//! functional mechanism.
//!
//! These routines will become invalid post EFI boot service exit.

use crate::efi::*;
use crate::efi::net;

/// The GUID of the PXE base code protocol.
pub const PXE_BASE_CODE_PROTOCOL_GUID: Guid = Guid::new(
    0x03C4E603, 0xAC28, 0x11d3, [0x9A,0x2D,0x00,0x90,0x27,0x3F,0xC1,0x4D]);

/// The current revision number of the PXE base code protocol.
pub const PXE_BASE_CODE_PROTOCOL_REVISION: Revision = 0x00010000;

#[derive(Debug, Copy, Clone)]
#[repr(C)]
/// This protocol is used to control PXE-compatible devices.
struct PxeBaseCodeProtocol {
    /// The revision of the PXE base code protocol.
    revision: Revision,

    start:          *const usize,
    stop:           *const usize,
    dhcp:           *const usize,
    discover:       *const usize,
    mftp:           *const usize,
    udp_write:      *const usize,
    udp_read:       *const usize,
    set_ip_filter:  *const usize,
    arp:            *const usize,
    set_parameters: *const usize,
    set_station_ip: *const usize,
    set_packets:    *const usize,
    mode:           *const PxeMode,
}

impl PxeBaseCodeProtocol {
    /// Returns a new empty `PxeBaseCodeProtocol`
    fn empty() -> Self {
        Self {
            revision:       PXE_BASE_CODE_PROTOCOL_REVISION,
            start:          core::ptr::null() as *const usize,
            stop:           core::ptr::null() as *const usize,
            dhcp:           core::ptr::null() as *const usize,
            discover:       core::ptr::null() as *const usize,
            mftp:           core::ptr::null() as *const usize,
            udp_write:      core::ptr::null() as *const usize,
            udp_read:       core::ptr::null() as *const usize,
            set_ip_filter:  core::ptr::null() as *const usize,
            arp:            core::ptr::null() as *const usize,
            set_parameters: core::ptr::null() as *const usize,
            set_station_ip: core::ptr::null() as *const usize,
            set_packets:    core::ptr::null() as *const usize,
            mode:           core::ptr::null() as *const PxeMode,
        }
    }
}

/// The maxmimum amount of entries in the `PxeMode` ARP cache
const MAX_ARP_ENTRIES: usize = 8;

/// The maxmium amount of entries in the `PxeMode` route cache
const MAX_ROUTE_ENTRIES: usize = 8;

#[derive(Clone)]
#[repr(C)]
/// PXE code mode.
///
/// All values in this structure are read-only and are updated by the code that
/// produces the [`PxeBaseCodeProtocol`] functions.
struct PxeMode {
    /// Whether this device has been started by calling `start()`
    started: bool,

    /// Whether the UNDI protocol supports IPv6
    ipv6_available: bool,

    /// Whether this PXE base code protocol implementation supports IPv6
    ipv6_supported: bool,

    /// Whether this device is currently using IPv6
    using_ipv6: bool,

    /// Whether this PXE base code implementation supports Boot Integrity
    /// Services
    bis_supported: bool,

    /// Whether this device and the platform support Boot Integrity Services
    bis_detected: bool,

    /// Whether ARP packets should be automatically generated
    auto_arp: bool,

    /// This field is used to change the chaddr field in the DHCP and Discovery
    /// packets. If `true`, `SystemGuid` (if one is available) will be sent.
    /// Otherwise the client NIC MAC address will be sent
    send_guid: bool,

    /// Whether the `dhcp()` function completed successfully
    dhcp_discover_valid: bool,

    /// Whether the `dhcp()` function completed successfully
    dhcp_ack_received: bool,

    /// Whether the `dhcp()` function completed successfully and a proxy DHCP
    /// offer packet was received
    proxy_offer_received: bool,

    /// When `true`, the `pxe_discover` packet field is valid
    pxe_discover_valid: bool,

    /// When `true`, the `pxe_reply` packet field is valid
    pxe_reply_received: bool,

    /// When `true`, the `pxe_bis_reply` packet field is valid
    pxe_bis_reply_received: bool,

    /// Indicates whether the `icmp_error` field has been updated
    icmp_error_received: bool,

    /// Indicates whether the `tftp_error` field has been updated
    tftp_error_received: bool,

    /// Whether callbacks should be made
    make_callbacks: bool,

    /// Time to live field of the IP header
    ttl: u8,

    /// Type of service field of the IP header
    tos: u8,

    /// The device's current IP address
    station_ip: net::IpAddr,

    /// The device's current subnet mask
    subnet_mask: net::IpAddr,

    /// Cached DHCP Discover packet
    dhcp_discover: net::DhcpPacket,

    /// Cached DHCP Ack packet
    dhcp_ack: net::DhcpPacket,

    /// Cached Proxy Offer packet
    proxy_offer: net::DhcpPacket,

    /// Cached PXE Discover packet
    pxe_discover: net::DhcpPacket,

    /// Cached PXE Reply packet
    pxe_reply: net::DhcpPacket,

    /// Cached PXE BIS Reply packet
    pxe_bis_reply: net::DhcpPacket,

    /// The current IP receive filter settings
    ip_filter: net::IpFilter,

    /// The number of valid entries in the ARP cache
    arp_cache_entries: u32,

    /// Array of cached ARP entries
    arp_cache: [net::ArpEntry; MAX_ARP_ENTRIES],

    /// The number of valid entries in the current route table
    route_table_entries: u32,

    /// Array of route table entries
    route_table: [net::RouteEntry; MAX_ROUTE_ENTRIES],

    /// ICMP error packet
    icmp_error: [u8; 504],

    /// TFTP error packet
    tftp_error: [u8; 128],
}

#[derive(Debug, Clone)]
struct PxeDevice {
    interface: PxeBaseCodeProtocol,
    handle: Handle,
}

impl PxeDevice {
    /// Attribute used to gain access to an interface by the handle protocol
    const BY_HANDLE: u32 = 0x00000001;

    /// Create a new `PxeInterface` for the `handle`
    fn new(handle: Handle) -> Self {
        // Create space for the interface that will be returned by the
        // `open_protocol()` call.
        let mut interface = core::ptr::null_mut();

        // Open the protocol for this handle
        let status = unsafe {
            (system_table().boot_svc.open_protocol)(
                handle,
                &PXE_BASE_CODE_PROTOCOL_GUID,
                &mut interface,
                *bootloader_image(),
                core::ptr::null_mut(),
                Self::BY_HANDLE,
            )
        };

        // Make sure we got it correctly
        assert!(status == Status::Success, "open protocol failed");

        // Cast the interface to the correct type
        let interface = unsafe {
            *(interface as *mut u8 as *mut PxeBaseCodeProtocol)
        };

        // Return it
        Self { interface, handle }
    }
}

impl Drop for PxeDevice {
    fn drop(&mut self) {
        // Close the protocol
        let status = unsafe {
            (system_table().boot_svc.close_protocol)(
                self.handle,
                &PXE_BASE_CODE_PROTOCOL_GUID,
                *bootloader_image(),
                core::ptr::null_mut(),
            )
        };
        assert!(status != Status::);
        assert!(status == Status::Success, "close protocol failed");
    }
}

pub fn download(kernel_filename: &str) {
    // Get the boot services required
    let locate_handle = system_table().boot_svc.locate_handle;

    // Call it once to get the size of the buffer required for all handles
    let mut handles: alloc::vec::Vec<Handle> = alloc::vec::Vec::new();
    let mut size = handles.len();

    unsafe {
        (locate_handle)(SearchType::ByProtocol,
                        &PXE_BASE_CODE_PROTOCOL_GUID,
                        core::ptr::null_mut(),
                        &mut size,
                        handles.as_mut_ptr());
    }

    // Extend the buffer to the required length
    handles.reserve_exact(size);

    unsafe {
        // Call it a second time to actually get the buffer of handles
        (locate_handle)(SearchType::ByProtocol,
                        &PXE_BASE_CODE_PROTOCOL_GUID,
                        core::ptr::null_mut(),
                        &mut size,
                        handles.as_mut_ptr());

        // Change the length of the vector to its actual size now
        handles.set_len(size / core::mem::size_of::<Handle>());
    }

    // Now we'll go through each handle, open the PXE protocol and check if the
    // device has been set up already. Because we use PXE to boot the
    // bootloader, there should be at least on device that will be already set
    // up.
    for handle in handles {
        // Open the device
        let device = PxeDevice::new(handle);

        // Get the device mode
        let mode = unsafe { &*(device.interface.mode) };

        // Check if it's been started and a DHCP ack has been received
        if mode.started && mode.dhcp_ack_received {
            // We found the device that likely booted our bootloader.
            // This is the one we're looking for
            println!("!!! Device likely found !!!");
            println!("{} | {} | {} | {} | {}",
                mode.started,
                mode.dhcp_ack_received,
                mode.dhcp_discover_valid,
                mode.pxe_discover_valid,
                mode.pxe_reply_received);
        }

        println!("------");
    }
}
