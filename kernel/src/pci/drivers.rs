//! All drivers this system is capable of handling.
//!
//! All drivers are required to implement the `Probe` function, which can then
//! be registered with the `register_driver!()` macro.
//!
//! The `register_driver!()` works by embedding all registered probe functions
//! within a link section of the kernel at compile time. These functions are
//! then invoked at runtime when pci devices are enumerated.

use alloc::sync::Arc;
use crate::pci;

/// Type used for PCI device probes to attempt to handle a device
/// The probe function type used for PCI device probes to attempt to handle a
/// device.
///
/// Each device driver must implement this probe function and register it with
/// `register_pci_driver!()`
pub type ProbePci = fn(&pci::DeviceConfig) -> Option<Arc<dyn Device>>;

/// Trait that all PCI device drivers must implement
pub trait Device: Send + Sync {
    /// Disable the device and release resources (called before a soft reboot)
    ///
    /// When a soft reboot happens, it is ideal to reset all devices to a state
    /// where they can be immediately re-initialized after the standard
    /// post-boot probe process.
    ///
    /// This function MUST BE RE-ENTRANT AT ALL COST.
    fn purge(&self);
}

/// This is the macro new drivers can be registered with. Simply call
/// `register_driver!(probe_fn)` and the driver will be registered within the
/// kernel!
#[macro_export] macro_rules! register_pci_driver {
    ($func:ident) => {
        const _: () = {
            #[used]
            #[unsafe(link_section = ".pci_probes")]
            static DRIVER: $crate::pci::ProbePci = $func;
        };
    }
}

unsafe extern "Rust" {
    static __start_pci_probes: ProbePci;
    static __end_pci_probes: ProbePci;
}

/// Return an array of all of the drivers registered with `register_driver!()`
/// within the kernel
pub fn get_pci_drivers() -> &'static [ProbePci] {
    unsafe {
        let start = &__start_pci_probes as *const ProbePci;
        let end = &__end_pci_probes as *const ProbePci;
        let count = end.offset_from(start) as usize;
        core::slice::from_raw_parts(start, count)
    }
}
