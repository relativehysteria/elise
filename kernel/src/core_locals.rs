/// A core-exclusive data structure which can be accessed via the `core!()`
/// macro.

use page_table::{VirtAddr, PhysAddr};

#[derive(Debug, Clone)]
pub struct CoreLocals {
    /// The address of this structure
    address: VirtAddr,

    /// A unique identifier allocated for this core
    pub id: u32,

    /// A reference to the data shared between the bootloader and the kernel
    pub shared: &'static shared_data::Shared,
}

/// Initialize the core locals for this core
pub fn init(shared: PhysAddr, core_id: u32) {
}
