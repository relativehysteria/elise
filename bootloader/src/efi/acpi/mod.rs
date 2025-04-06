//! ACPI definitions

pub mod acpi;
pub mod apic;

pub use acpi::*;

#[derive(Debug)]
/// Errors possibly returned by ACPI routines
pub enum Error {
    /// ACPI 2.0 table couldn't be found
    Acpi20NotFound,

    /// Unexpected table signature
    SignatureMismatch(Table),

    /// Unexpected table size
    SizeMismatch(Table),

    /// Unexpected table checksum
    ChecksumMismatch(Table),

    /// While parsing the memory proximity domain to physical memory ranges
    /// affinity, the physical memory range was larger than a `usize`.
    MemoryAffinityOverflow,
}
