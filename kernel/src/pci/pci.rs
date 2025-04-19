//! Routines for the handling of PCI devices

// PCIe might be implemented later if needed

use alloc::vec::Vec;
use alloc::boxed::Box;
use core::mem::size_of;
use core::fmt::Debug;
use spinlock::SpinLock;
use crate::pci::DRIVERS;
use crate::core_locals::InterruptLock;

/// I/O port for the configuration space address
const PCI_CONFIG_ADDRESS: *const u32 = 0xCF8 as *const u32;

/// I/O port for the configuration space data
const PCI_CONFIG_DATA: *const u32 = 0xCFC as *const u32;

/// List of devices handled by a driver
static DEVICES: SpinLock<Vec<Box<dyn Device>>, InterruptLock> =
    SpinLock::new(Vec::new());

#[derive(Clone, Copy, Debug)]
#[repr(C)]
#[allow(missing_docs)]
/// PCI header common for any other PCI header
pub struct Header {
    pub vendor_id:       u16,
    pub device_id:       u16,
    pub command:         u16,
    pub status:          u16,
    pub revision:        u8,
    pub prog_if:         u8,
    pub subclass:        u8,
    pub class:           u8,
    pub cache_line_size: u8,
    pub latency_timer:   u8,
    pub header_type:     u8,
    pub bist:            u8,
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
#[allow(missing_docs)]
/// Configuration of a PCI device
pub struct DeviceConfig {
    pub header:                Header,
    pub bar0:                  u32,
    pub bar1:                  u32,
    pub bar2:                  u32,
    pub bar3:                  u32,
    pub bar4:                  u32,
    pub bar5:                  u32,
    pub cardbus_cis_pointer:   u32,
    pub subsystem_vendor_id:   u16,
    pub subsystem_device_id:   u16,
    pub expansion_rom_address: u32,
    pub capabilities:          u8,
    pub reserved:              [u8; 7],
    pub interrupt_line:        u8,
    pub interrupt_pin:         u8,
    pub min_grant:             u8,
    pub max_latency:           u8,
}

impl DeviceConfig {
    /// Returns the string representation of the header and subsystem vendor and
    /// device IDs
    pub fn did_vid(&self) -> alloc::string::String {
        alloc::format!("{:#06x}:{:#06x} {:#06x}:{:#06x}",
                       self.header.vendor_id,
                       self.header.device_id,
                       self.subsystem_vendor_id,
                       self.subsystem_device_id)
    }
}

/// Trait that all PCI device drivers must implement
pub trait Device: Send + Sync {
    /// Disable the device and release resources (called before a soft reboot)
    ///
    /// When a soft reboot happens, it is ideal to reset all devices to a state
    /// where they can be immediately re-initialized after the standard
    /// post-boot probe process.
    ///
    /// This function MUST BE RE-ENTRANT AT ALL COST.
    fn purge(&mut self);
}

/// Trait that devices can implement to be registered during the PCI probe
/// process
pub trait Driver: Send + Sync + Debug {
    /// Probes the device configuration and returns an initialized device if
    /// supported
    fn probe(&self, cfg: &DeviceConfig) -> Option<Box<dyn Device>>;
}

/// Enumerate all available PCI devices on the system and initialize their
/// drivers if supported
pub unsafe fn init() {
    // Chances of the devices changing between soft reboots are pretty much
    // close to none, so we might want to save the device BDF IDs in a bitmap
    // and instead of going over the devices again, just go through the bitmap.

    // For each bus ID
    (0..256).flat_map(|bus| {
        // For each device ID
        (0..32).flat_map(move |device| {
            // For each function ID
            (0..8).map(move |function| (bus, device, function))
        })
    }).for_each(|(bus, device, function)| {
        // Compute the address for this Bus:Device.Function
        let pci_addr = (bus << 8) | (device << 3) | (function << 0);

        // Compute the PCI selection address for this BDF
        let select_addr = (1 << 31) | (pci_addr << 8);

        // Read the device and vendor IDs
        unsafe { cpu::out32(PCI_CONFIG_ADDRESS, select_addr); }
        let did_vid = unsafe { cpu::in32(PCI_CONFIG_DATA) };

        // If no device is registered, go next
        if did_vid == u32::MAX { return; }

        let header = unsafe { read_pci_registers::<Header>(select_addr) };

        // Skip non-device entries (bridges etc.)
        if (header.header_type & 0x7F) != 0 { return; }

        // Read the device config
        let dev_cfg = unsafe {
            read_pci_registers::<DeviceConfig>(select_addr)
        };

        // If we found at least one driver, prevent the "no driver" message from
        // popping out during further probes
        let mut msg_printed = false;

        // If we have a driver registered for this device, save the device
        for driver in DRIVERS {
            if let Some(device) = driver.probe(&dev_cfg) {
                print!("PCI driver for dev: {} > ", dev_cfg.did_vid());
                println!("{driver:?} | INITIALIZING DEVICE");
                DEVICES.lock().push(device);
                msg_printed = true;
            } else if !msg_printed {
                println!("No driver for dev: {}", dev_cfg.did_vid());
                msg_printed = true;
            }
        }
    });
}

/// Reset all devices on the system such that they must me reinitialized through
/// `init()` before use
pub unsafe fn reset_devices() {
    unimplemented!();
}

/// Read a struct `T` given the PCI `select_addr`
///
/// It is up to the caller to ensure the type `T` has the correct size and
/// alignment (multiple of u32 and alignment at most u32).
unsafe fn read_pci_registers<T>(select_addr: u32)
        -> T where [u32; size_of::<T>() / size_of::<u32>()]: {
    // Create array to hold the register data
    let mut data = [0u32; size_of::<T>() / size_of::<u32>()];

    for (idx, register) in data.iter_mut().enumerate() {
        // Set the window to the selected register
        unsafe {
            cpu::out32(PCI_CONFIG_ADDRESS,
                select_addr | (idx * size_of::<u32>()) as u32);
        }

        // Read the value
        *register = unsafe { cpu::in32(PCI_CONFIG_DATA) };
    }

    // Transmute the array into the desired type
    let data = unsafe { core::ptr::read_unaligned(data.as_ptr() as *const T) };
    data
}
