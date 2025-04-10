//! Routines for the parsing of ACPI tables and such

// THIS CODE IS A DIRT TURNED TO GLASS WASTELAND FIT FOR THE KING OF KINGS:
// LOOK ON MY WORKS, YE MIGHTY, AND DESPAIR

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::mem::size_of;
use core::ptr::read_unaligned;

use rangeset::{ RangeSet, Range };
use page_table::PhysAddr;
use crate::acpi::Error;
use crate::acpi::apic;
use crate::mm::phys_ptr;

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
            b"APIC" => Self::Madt,
            b"SRAT" => Self::Srat,
            _______ => Self::Unknown(*signature),
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


/// Flag showing that this table entry is enabled
const ENABLED: u32 = 1 << 0;

/// Flag showing that the APIC is online capable
const ONLINE_CAPABLE: u32 = 1 << 1;

unsafe fn parse_srat(hdr_ptr: *const SdtHeader)
        -> Result<(BTreeMap<u32, u32>, BTreeMap<u32, RangeSet>), Error> {
    // Get the entries from the table
    let entries = unsafe {
        parse_table_entries(hdr_ptr, size_of::<[u8; 12]>())?
    };

    // APIC to memory proximity domain affinity
    let mut apic_to_domain = BTreeMap::new();

    // memory proximity domain to physical memory ranges affinity
    let mut domain_to_ranges = BTreeMap::new();

    // The error we will return if the entry's length doesn't match the expected
    let mismatch_err = Err(Error::SizeMismatch(Table::Srat));

    // Go through each entry and save each usable affinity
    for entry in entries {
        match entry.typ {
            // Local APIC to memory proximity domain
            0 => {
                // Validate the length of the entry
                if entry.len != 16 { return mismatch_err; }

                // Extract the fields we care about
                let id = entry.read::<u8>(3) as u32;
                let flags = entry.read::<u32>(4);
                let domain = u32::from_le_bytes([
                    entry.read::<u8>(2),
                    entry.read::<u8>(9),
                    entry.read::<u8>(10),
                    entry.read::<u8>(11),
                ]);

                // Save the affinity if enabled
                if (flags & ENABLED) != 0 {
                    apic_to_domain.insert(id, domain);
                }
            },
            1 => {
                // Validate the length of the entry
                if entry.len != 40 { return mismatch_err; }

                // Extract the fields we care about
                let domain = entry.read::<u32>(2);
                let start = entry.read::<usize>(8);
                let length = entry.read::<u64>(16) as usize;
                let flags = entry.read::<u32>(28);

                // Save the affinity if enabled
                if length > 0 && (flags & ENABLED) != 0 {
                    // XXX: Technically this should be an underflow but suck my
                    // dick
                    let end = length.checked_sub(1)
                        .ok_or(Error::MemoryAffinityOverflow)?;
                    let end = start.checked_add(end)
                        .ok_or(Error::MemoryAffinityOverflow)?;

                    // XXX: And this should be a rangeset error but again,
                    // suck this cock clean
                    domain_to_ranges.entry(domain)
                        .or_insert(RangeSet::new())
                        .insert(Range::new(start as u64, end as u64).unwrap())
                        .map_err(|_| Error::MemoryAffinityOverflow)?;
                }
            },
            2 => {
                // Validate the length of the entry
                if entry.len != 24 { return mismatch_err; }

                // Extract the fields we care about
                let domain = entry.read::<u32>(4);
                let id = entry.read::<u32>(8);
                let flags = entry.read::<u32>(12);

                // Save the affinity if enabled
                if (flags & ENABLED) != 0 {
                    apic_to_domain.insert(id, domain);
                }
            },
            _ => unimplemented!(),
        }
    }

    Ok((apic_to_domain, domain_to_ranges))
}

unsafe fn parse_madt(hdr_ptr: *const SdtHeader) -> Result<Vec<u32>, Error> {
    // Get the entries from the table
    let entries = unsafe {
        parse_table_entries(hdr_ptr, 2 * size_of::<u32>())?
    };

    // ID vector of all usable APICs
    let mut apics = Vec::new();

    // The error we will return if the entry's length doesn't match the expected
    let mismatch_err = Err(Error::SizeMismatch(Table::Madt));

    // Go through each entry and save the IDs of functional APICs
    for entry in entries {
        match entry.typ {
            // Local APIC
            0 => {
                // Validate the length of the entry
                if entry.len != 8 { return mismatch_err; }

                // Read the APIC ID and flags
                let id    = entry.read::<u8>(3) as u32;
                let flags = entry.read::<u32>(4);

                // If the CPU is enabled, or can be enabled, save the ID
                if (flags & ENABLED) != 0 || (flags & ONLINE_CAPABLE) != 0 {
                    apics.push(id);
                }
            },

            // Local x2APIC
            9 => {
                // Validate the length of the entry
                if entry.len != 16 { return mismatch_err; }

                // Read the APIC ID and flags
                let id    = entry.read::<u32>(4);
                let flags = entry.read::<u32>(8);

                // If the CPU is enabled, or can be enabled, save the ID
                if (flags & ENABLED) != 0 || (flags & ONLINE_CAPABLE) != 0 {
                    apics.push(id);
                }
            },
            _ => {},
        }
    }

    Ok(apics)
}

/// Represents an entry in an ACPI table
struct TableEntry {
    typ: u8,
    len: u8,
    ptr: *const u8,
}

impl TableEntry {
    /// Read a value of type `T` from the entry at the given offset
    fn read<T>(&self, offset: usize) -> T {
        unsafe { read_unaligned(self.ptr.add(offset) as *const T) }
    }
}

/// Parse the entries in an ACPI table, given the pointer to the table and the
/// offset to the first entry.
unsafe fn parse_table_entries(hdr_ptr: *const SdtHeader, offset: usize)
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
        let typ = unsafe { read_unaligned(ptr.add(0) as *const u8) };
        let len = unsafe { read_unaligned(ptr.add(1) as *const u8) };

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
    // APIC IDs
    let mut apics: Option<Vec<u32>> = None;

    // APIC to memory domain
    let mut apic_domains: Option<apic::ApicDomains> = None;

    // memory domain to physical memory ranges
    let mut mem_domains: Option<apic::MemoryDomains> = None;

    // Get the physical pointer to the SDTs and offset it into our phys window
    let sdt_table = core!().shared.acpi().get().clone();
    let base = phys_ptr(sdt_table.base).0;

    // Go through each SDT and parse it
    for entry in 0..sdt_table.n_entries {
        // Get the pointer to the table
        let offset = (entry as usize).checked_mul(size_of::<u64>())
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
                apics = unsafe { Some(parse_madt(table_ptr)?) };
            },
            Table::Srat => {
                let (ad, md) = unsafe { parse_srat(table_ptr)? };
                apic_domains = Some(ad);
                mem_domains  = Some(md);
            },
            _ => continue,
        }
    }

    // Initialize the APIC states and NUMA topologies
    apic::init(apics, apic_domains, mem_domains)?;

    Ok(())
}
