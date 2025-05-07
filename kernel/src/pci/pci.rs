//! Routines for the handling of PCI devices

// PCIe might be implemented later if needed

use alloc::vec::Vec;
use alloc::sync::Arc;
use core::mem::size_of;
use core::fmt::Debug;

use spinlock::SpinLock;

use crate::core_locals::InterruptLock;

/// I/O port for the configuration space address
const PCI_CONFIG_ADDRESS: u16 = 0xCF8;

/// I/O port for the configuration space data
const PCI_CONFIG_DATA: u16 = 0xCFC;

/// List of devices handled by a driver
static DEVICES: SpinLock<Vec<Arc<dyn crate::pci::Device>>, InterruptLock> =
    SpinLock::new(Vec::new());

/// PCI header common for any other PCI header
#[derive(Clone, Copy, Debug)]
#[repr(C)]
#[allow(missing_docs)]
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

/// Configuration of a PCI device
#[derive(Clone, Copy, Debug)]
#[repr(C)]
#[allow(missing_docs)]
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

/// The bitness of a BAR
#[derive(Debug, PartialEq)]
#[allow(missing_docs)]
pub enum BarBits {
    Bit32,
    Bit64,
}

impl BarBits {
    /// Return the bitness of `bar`
    pub fn from_bar(bar: u32) -> Self {
        match (bar >> 1) & 0b11 {
            0b10 => Self::Bit64,
            _    => Self::Bit32,
        }
    }

    /// Based on the bitness of `bar0`, returns a whole `u64` value from
    /// either `bar0` (if 32-bit), or in `bar0 << 32 | bar1` (if 64-bit)
    /// with the BAR type bits masked off from both
    pub fn u64(bar0: u32, bar1: u32) -> u64 {
        // Mask off the bits from both
        let lower  = (bar0 & !0b1111) as u64;
        let higher = (bar1 & !0b1111) as u64;

        // Return the u64 based on bitness
        match Self::from_bar(bar0) {
            Self::Bit32 => lower,
            Self::Bit64 => (higher << 32) | lower,
        }
    }
}

/// The memory type of a BAR
#[derive(Debug, PartialEq)]
#[allow(missing_docs)]
pub enum BarType {
    IO,
    Memory,
}

impl BarType {
    /// Return the type of `bar`
    pub fn from_bar(bar: u32) -> Self {
        match bar & 0b1 {
            0 => Self::Memory,
            _ => Self::IO,
        }
    }
}

/// Enumerate all available PCI devices on the system and initialize their
/// drivers if supported
pub unsafe fn init() {
    // Chances of the devices changing between sof reboots are pretty much
    // close to none, so we might want to save the device BDF IDs instead of
    // looping over them again

    // Get the drivers registered in the kernel
    let drivers = crate::pci::get_pci_drivers();

    // For each bus ID
    (0..256).flat_map(|bus| {
        // For each device ID
        (0..32).flat_map(move |device| {
            // For each function ID
            (0..8).map(move |function| (bus, device, function))
        })
    }).for_each(|(bus, device, function)| {
        // Compute the address for this Bus:Device.Function
        let pci_addr = (bus << 8) | (device << 3) | function;

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

        // If we have a driver registered for this device, save the device
        for probe in drivers {
            if let Some(device) = probe(&dev_cfg) {
                println!("Got PCI driver for device: {} ", dev_cfg.did_vid());
                DEVICES.lock().push(device);
            }
        }
    });

    // This is a post-probe hook. If things get more complicated, it might be
    // better to actually create a hook-register, but this works for now
    finish_probes();
}

/// Post-probe hook. Will run after all the PCI devices are probed
fn finish_probes() {
    // Lock in net devices
    crate::net::NetDevice::lock_in();
}

/// Reset all devices on the system such that they must be reinitialized through
/// `init()` before use
pub unsafe fn reset_devices() {
    unsafe { &mut *DEVICES.shatter() }
        .into_iter()
        .for_each(|dev| dev.purge());
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
    unsafe { core::ptr::read_unaligned(data.as_ptr() as *const T) }
}
