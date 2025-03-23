/// A core-exclusive data structure which can be accessed via the `core!()`
/// macro.

use page_table::{VirtAddr, PhysAddr};
use crate::SHARED;

#[derive(Debug, Clone)]
pub struct CoreLocals {
    /// The address of this structure
    address: VirtAddr,

    /// A unique identifier allocated for this core
    pub id: u32,
}

/// Initialize the core locals for this core
pub fn init(core_id: u32) {}
