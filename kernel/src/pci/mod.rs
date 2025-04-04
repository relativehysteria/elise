//! Drivers and other routines related to PCI-based devices

mod pci;

pub use pci::*;

/// List of drivers that will be probed during PCI enumeration
pub static DRIVERS: &[&dyn pci::Driver] = &[];
