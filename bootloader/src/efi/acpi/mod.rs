//! ACPI definitions

pub mod acpi;

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
}
