//! Routines for the parsing of ACPI tables and such

use alloc::vec::Vec;
use core::mem::size_of;
use core::ptr::read_unaligned;
use page_table::PhysAddr;
use crate::acpi::{Error, Madt, Srat};
use crate::apic;
use crate::mm::{phys_ptr, register_numa};

/// Flag showing that a table entry is enabled
pub const ENABLED: u32 = 1 << 0;

#[derive(Debug, PartialEq)]
/// Types of tables recognized by this lib -- used for error handling
pub enum Table {
    /// Multiple APIC description table
    Madt,

    /// System resource affinity table
    Srat,

    /// Unknown system table
    Unknown([u8; 4]),
}

impl Table {
    /// Tries to turn a signature into a recognized table type
    pub fn from_sig(signature: &[u8; 4]) -> Self {
        match signature {
            b"APIC"  => Self::Madt,
            b"SRAT"  => Self::Srat,
            _unknown => Self::Unknown(*signature),
        }
    }
}

#[derive(Debug)]
#[repr(C)]
/// Header present in all SDTs
pub struct SdtHeader {
    /// ASCII string representation of the table identifier
    pub signature: [u8; 4],

    /// Length of the table, in bytes, including the header, starting from
    /// offset 0
    pub length: u32,

    /// Revision of the structure corresponding to the signature of this table
    pub revision: u8,

    /// The entire table, including the checksum field, must add to zero to be
    /// considered valid
    pub checksum: u8,

    /// OEM-supplied string that identifies the OEM
    pub oem_id: [u8; 6],

    /// OEM-supplied string that the OEM uses to identify this table
    pub oem_table_id: [u8; 8],

    /// OEM-supplied revision number
    pub oem_revision: u32,

    /// Vendor ID of utility that created the table
    pub creator_id: u32,

    /// Revision of utility that created the table
    pub creator_revision: u32,
}

impl SdtHeader {
    /// Checks the checksum of the header _AND_ the connected table
    pub fn checksum_valid(&self) -> bool {
        let bytes = unsafe { core::slice::from_raw_parts(
                self as *const Self as *const u8, self.length as usize) };
        let sum = bytes.iter().fold(0u8, |x, &byte| x.wrapping_add(byte));
        sum == 0
    }
}

/// Represents an entry in an ACPI table
pub struct TableEntry {
    pub typ: u8,
    pub len: u8,
    ptr: *const u8,
}

impl TableEntry {
    /// Read a value of type `T` from the entry at the given offset
    pub fn read<T>(&self, offset: usize) -> T {
        unsafe { read_unaligned(self.ptr.add(offset) as *const T) }
    }
}

/// Parse the entries in an ACPI table, given the pointer to the table and the
/// offset to the first entry.
pub unsafe fn parse_table_entries(hdr_ptr: *const SdtHeader, offset: usize)
        -> Result<Vec<TableEntry>, Error> {
    // Some pointer fuckery; cast the pointer as byte pointer so it doesn't
    // realign on its own and get a usable rust reference to the header
    let hdr_bytes = hdr_ptr as *const u8;
    let hdr = unsafe { &*hdr_ptr };

    // Get the base pointer of the entries and the end of the table
    let mut ptr = unsafe { hdr_bytes.add(size_of::<SdtHeader>() + offset) };
    let end = unsafe { hdr_bytes.add(hdr.length as usize) };

    // Get the type of the table we're parsing so we can return useful errors
    let table_type = Table::from_sig(&hdr.signature);

    // Vector of parsed entries
    let mut entries = Vec::new();

    // Go through each entry and get its type, its length and the pointer to it
    while ptr < end {
        let typ = unsafe { read_unaligned(ptr.add(0)) };
        let len = unsafe { read_unaligned(ptr.add(1)) };

        // Make sure there's space for the entry
        if len < 2 { return Err(Error::SizeMismatch(table_type)); }

        // Save the entry
        entries.push(TableEntry { typ, len, ptr });

        // Offset the current entry pointer to the next one and go
        ptr = unsafe { ptr.add(len as usize) };
    }

    Ok(entries)
}

/// Initialize the ACPI tables
pub unsafe fn init() -> Result<(), Error> {
    // Parsed table information
    let mut madt: Option<Madt> = None;
    let mut srat: Option<Srat> = None;

    // Get the physical pointer to the SDTs and offset it into our phys window
    let sdt_table = *core!().shared.acpi().get();
    let base = phys_ptr(sdt_table.base).0;

    // Go through each SDT and parse it
    for entry in 0..sdt_table.n_entries {
        // Get the pointer to the table
        let offset = entry.checked_mul(size_of::<u64>())
            .expect("Overflow when offseting into physical window");
        let table_ptr = (base as usize).checked_add(offset)
            .expect("Base virtual address overflow");

        // Read the physical address of the SDT
        let table_ptr = unsafe {
            read_unaligned(table_ptr as *const *const SdtHeader)
        };

        // Offset the pointer to our physical window
        let table_ptr = phys_ptr(PhysAddr(table_ptr as u64));
        let table_ptr = table_ptr.0 as *const SdtHeader;

        // Get the signature for the table
        let signature = unsafe { read_unaligned(table_ptr as *const [u8; 4]) };

        // Print out the table that we're reading right now
        if let Ok(sig) = core::str::from_utf8(&signature) {
            let table = Table::from_sig(&signature);
            println!("Got ACPI table: {sig} | {table:?}");
        }

        // Parse the table
        match Table::from_sig(&signature) {
            Table::Madt => {
                madt = unsafe { Some(Madt::parse(table_ptr)?) };
            },
            Table::Srat => {
                srat = unsafe { Some(Srat::parse(table_ptr)?) };
            },
            _ => continue,
        }
    }

    // Inform the memory manager of our NUMA topology
    if let Some(srat) = srat {
        unsafe { register_numa(srat.apic_to_domain, srat.domain_to_ranges); }
    }

    // Initialize the APIC states on the system and bring up the other cores
    if let Some(madt) = madt {
        apic::init_system(madt.apics)?;
    }

    Ok(())
}
