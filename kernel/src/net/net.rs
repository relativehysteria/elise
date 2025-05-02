//! The network driver abstraction trait that has to be implemented by all NIC
//! drivers in the kernel

#[derive(Debug, Clone, PartialEq, Eq)]
#[repr(transparent)]
/// The MAC address of a NIC
pub struct Mac(pub [u8; 6]);

/// The driver trait that allows access to NIC RX and TX
pub trait NetDriver: Send + Sync {
    /// Forcibly reset the NIC
    unsafe fn reset(&self);

    /// Get the MAC address of the NIC
    fn mac(&self) -> Mac;
}
