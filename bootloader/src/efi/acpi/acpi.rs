//! Routines for the parsing of ACPI tables and such

use core::mem::size_of;
use shared_data::SdtTable;
use page_table::PhysAddr;
use crate::efi::{Guid, SystemTablePtr, ConfigTable};
use crate::efi::acpi::Error;

/// GUID of the ACPI 2.0 table
pub const ACPI_20_TABLE_GUID: Guid = Guid::new(
    0x8868e871,0xe4f1,0x11d3, [0xbc,0x22,0x00,0x80,0xc7,0x3c,0x88,0x81]);

/// Types of tables recognized by this lib -- used for error handling
#[derive(Debug, PartialEq)]
pub enum Table {
    /// Root system descriptor pointer
    Rsdp,

    /// Extended root system descriptor table
    Xsdt,

    /// Unknown system table
    Unknown([u8; 4]),
}

impl Table {
    /// Tries to turn a signature into a recognized table type
    pub fn from_sig(signature: &[u8; 4]) -> Self {
        match signature {
            b"XSDT" => Self::Xsdt,
            _______ => Self::Unknown(*signature),
        }
    }
}

/// Root system descriptor pointer
#[derive(Debug)]
#[repr(packed)]
struct Rsdp {
    /// `RSD PTR `
    signature: [u8; 8],

    /// Checksum of the fields defined in ACPI 1.0. Must sum to 0
    _checksum: u8,

    /// OEM-supplied string that identifies the OEM
    _oemid: [u8; 6],

    /// Revision of this structure. Backward compatible
    _revision: u8,

    /// Physical address of the RDST
    _rsdt_addr: u32,

    /// Length of the table, in bytes, including the header, starting from
    /// add 0
    length: u32,

    /// Physical address of the XSDT
    xsdt_addr: u64,

    /// Checksum of the entire table, including both checksum fields
    _extended_checksum: u8,
}

impl Rsdp {
    /// Returns a validated RSDP pointer
    pub unsafe fn from_system_table(system_table: SystemTablePtr)
            -> Result<*const Self, Error> {
        // Transmute the pointer as an array of tables
        let tables = unsafe {
            let system_table = &*system_table.0;
            core::slice::from_raw_parts(
                system_table.cfg_tables,
                system_table.n_cfg_entries)
        };

        // Get the RSDP
        let rsdp = unsafe {
            &*(tables.iter().find_map(|ConfigTable { guid, table }| {
                (*guid == ACPI_20_TABLE_GUID)
                    .then_some(*table)
            }).ok_or(Error::Acpi20NotFound)? as *const Self)
        };

        // Verify the length
        if (rsdp.length as usize) < size_of::<Rsdp>() {
            return Err(Error::SizeMismatch(Table::Rsdp));
        }

        // Verify the signature
        if &rsdp.signature != b"RSD PTR " {
            return Err(Error::SignatureMismatch(Table::Rsdp));
        }

        // Verify the checksum
        let bytes = unsafe { core::slice::from_raw_parts(
                rsdp as *const Self as *const u8, rsdp.length as usize) };
        let sum = bytes.iter().fold(0u8, |x, &byte| x.wrapping_add(byte));

        if sum != 0 {
            return Err(Error::ChecksumMismatch(Table::Rsdp));
        }

        // Return the pointer
        Ok(rsdp as *const Self)
    }
}

/// Header present in all SDTs
#[derive(Debug)]
#[repr(C)]
struct SdtHeader {
    /// ASCII string representation of the table identifier
    signature: [u8; 4],

    /// Length of the table, in bytes, including the header, starting from
    /// add 0
    length: u32,

    /// Revision of the structure corresponding to the signature of this table
    revision: u8,

    /// The entire table, including the checksum field, must add to zero to be
    /// considered valid
    checksum: u8,

    /// OEM-supplied string that identifies the OEM
    oem_id: [u8; 6],

    /// OEM-supplied string that the OEM uses to identify this table
    oem_table_id: [u8; 8],

    /// OEM-supplied revision number
    oem_revision: u32,

    /// Vendor ID of utility that created the table
    creator_id: u32,

    /// Revision of utility that created the table
    creator_revision: u32,
}

impl SdtHeader {
    /// Checks the checksum of the header _AND_ the connected table
    fn checksum_valid(&self) -> bool {
        let bytes = unsafe { core::slice::from_raw_parts(
                self as *const Self as *const u8, self.length as usize) };
        let sum = bytes.iter().fold(0u8, |x, &byte| x.wrapping_add(byte));
        sum == 0
    }
}

pub unsafe fn get_sdt_table(sys: SystemTablePtr) -> Result<SdtTable, Error> {
    // Get the RSDP
    let rsdp = unsafe { &*(Rsdp::from_system_table(sys)?) };

    // Get the XSDT pointer
    let xsdt = unsafe { &*(rsdp.xsdt_addr as *const SdtHeader) };

    // Verify the signature
    if Table::from_sig(&xsdt.signature) != Table::Xsdt {
        return Err(Error::SignatureMismatch(Table::Xsdt));
    }

    // Verify the checksum
    if !xsdt.checksum_valid() {
        return Err(Error::ChecksumMismatch(Table::Xsdt))
    }

    // Get the pointer to the SDT table
    let base = (xsdt as *const SdtHeader as usize) + size_of::<SdtHeader>();
    let base = PhysAddr(base as u64);

    // Get the amount of entries in the SDT table
    let n_entries = (xsdt.length as usize)
        .checked_sub(size_of::<SdtHeader>())
        .expect("Integer underflow on table length")
        .checked_div(size_of::<u64>())
        .unwrap();

    Ok(SdtTable { n_entries, base })
}
