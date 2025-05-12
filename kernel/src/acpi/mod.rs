//! ACPI definitions

mod acpi;
mod madt;
mod srat;

pub use srat::*;
pub use madt::*;
pub use acpi::*;

/// Errors possibly returned by ACPI routines
#[derive(Debug)]
pub enum Error {
    /// ACPI 2.0 table couldn't be found
    Acpi20NotFound,

    /// Unexpected table signature
    SignatureMismatch(Table),

    /// Unexpected table size
    SizeMismatch(Table),

    /// Unexpected table checksum
    ChecksumMismatch(Table),

    /// We got flags that the kernel can't handled
    UnhandledFlags,

    /// While parsing the memory proximity domain to physical memory ranges
    /// affinity, the physical memory range was larger than a `usize`.
    MemoryAffinityOverflow,
}
