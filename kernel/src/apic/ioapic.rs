//! IO APIC implementation

/// IO APIC
pub struct IoApic {
    /// The ID of this IO APIC
    id: u8,

    /// The MMIO address of this IO APIC
    addr: u32,

    /// The Global System Interrupt base of this IO APIC
    gsi: u32,
}

impl IoApic {
    /// Create a new IO APIC
    pub fn new(id: u8, addr: u32, gsi: u32) -> Self {
        Self { id, addr, gsi }
    }
}
