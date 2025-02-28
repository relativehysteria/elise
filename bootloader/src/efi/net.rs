//! Network protocol routines and structures that utilize the EFI boot services
//! as its functional mechanism.
//!
//! These routines will become invalid post EFI boot service exit.

#![allow(dead_code)]

#[derive(Clone, Copy)]
/// A MAC address
pub struct Mac([u8; 32]);

#[derive(Clone, Copy)]
/// An IP version 4 address
pub struct Ipv4([u8; 4]);

#[derive(Clone, Copy)]
/// An IP version 6 address
pub struct Ipv6([u8; 16]);

#[derive(Clone, Copy)]
#[repr(C)]
/// Represents an IP address
pub union IpAddr {
    raw: [u32; 4],
    v4: Ipv4,
    v6: Ipv6,
}

#[derive(Clone, Copy)]
#[repr(packed)]
/// Represents a DHCPv4 packet.
///
/// All multibyte fields are stored in network order (big endian).
pub struct DhcpV4 {
    /// The operation code
    bootp_op_code: u8,

    /// The hardware type
    bootp_hw_type: u8,

    /// Length of the hardware address
    bootp_hw_addr_len: u8,

    /// The gateway hops (number of hops for the packet)
    bootp_gate_hops: u8,

    /// Unique identifier for the DHCP request
    bootp_ident: u32,

    /// Seconds elapsed since the client began the DHCP discovery process
    bootp_seconds: u16,

    /// Flags for the DHCP message (e.g., broadcast flag)
    bootp_flags: u16,

    /// Client IP address (for DHCP discover)
    bootp_ci_addr: [u8; 4],

    /// Your (client) IP address assigned by the server
    bootp_yi_addr: [u8; 4],

    /// Server IP address
    bootp_si_addr: [u8; 4],

    /// Gateway IP address
    bootp_gi_addr: [u8; 4],

    /// Client hardware address (usually MAC address)
    bootp_hw_addr: [u8; 4],

    /// Server name
    bootp_srv_name: [u8; 64],

    /// Boot file name
    bootp_boot_file: [u8; 128],

    /// DHCP magic number to identify the packet as a valid DHCP message
    dhcp_magik: u32,

    /// Additional DHCP options
    dhcp_options: [u8; 56],
}

#[derive(Clone, Copy)]
#[repr(packed)]
/// Represents a DHCPv4 packet
///
/// All multibyte fields are stored in network order (big endian).
pub struct DhcpV6 {
    /// The message type
    message_type: u8,

    /// The transaction ID
    transaction_id: [u8; 3],

    /// DHCPv6 options
    dhcp_options: [u8; 1024],
}

#[derive(Clone, Copy)]
#[repr(packed)]
pub union DhcpPacket {
    /// Raw byte array representation of the DHCP packet
    raw: [u8; 1472],

    /// IPv4-specific DHCP packet
    v4: DhcpV4,

    /// IPv6-specific DHCP packet
    v6: DhcpV6,
}

/// The maximum amount of IPs in the `ip_list` field of the [`IpFilter`] struct
const MAX_IP_COUNT: usize = 8;

#[derive(Clone, Copy)]
#[repr(C)]
/// Represents a PXE IP filter
pub struct IpFilter {
    /// Flags or attributes for the filter (e.g., whether to block or allow
    /// certain IPs)
    filters: u8,

    /// The number of IPs in the ip_list
    ip_count: u8,

    /// Reserved, unused space
    reserved: u16,

    /// List of IP addresses for filtering
    ip_list: [IpAddr; MAX_IP_COUNT],
}

#[derive(Clone, Copy)]
#[repr(C)]
/// Represents an entry into the ARP cache table
pub struct ArpEntry {
    /// The IP address in the ARP cache
    ip_addr: IpAddr,

    /// The corresponding MAC address in the ARP cache
    mac_addr: Mac,
}

#[derive(Clone, Copy)]
#[repr(C)]
/// Represents an entry into the route table
pub struct RouteEntry {
    /// The destination network IP address
    ip_addr: IpAddr,

    /// The subnet mask for the destination network
    subnet_mask: IpAddr,

    /// The gateway address for routing packets to the destination network
    gw_addr: IpAddr,
}
