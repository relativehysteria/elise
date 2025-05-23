//! L2: IPv4/v6 implementation
mod v4;
mod v6;
mod ip;

pub use v4::*;
pub use v6::*;
pub use ip::*;

/// IP transport protocol
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
#[repr(u8)]
pub enum TransportProtocol {
    Icmp = 0x01,
    Tcp  = 0x06,
    Udp  = 0x11,
}
