//! SRAT implementation

use alloc::collections::BTreeMap;

use rangeset::{Range, RangeSet};

use crate::acpi::{SdtHeader, Error, Table, ENABLED, parse_table_entries};

/// Information returned when parsing the SRAT table
pub struct Srat {
    /// Mapping of APICs to memory domains
    pub apic_to_domain: BTreeMap<u32, u32>,

    /// Mapping of memory domains to specific memory regions
    pub domain_to_ranges: BTreeMap<u32, RangeSet>,
}

impl Srat {
    pub unsafe fn parse(hdr_ptr: *const SdtHeader) -> Result<Self, Error> {
        // Get the entries from the table
        let entries = unsafe {
            parse_table_entries(hdr_ptr, size_of::<[u8; 12]>())?
        };

        // Create the SRAT struct that will be returned
        let mut srat = Self {
            apic_to_domain: BTreeMap::new(),
            domain_to_ranges: BTreeMap::new(),
        };

        // The error returned if the entry's length doesn't match the spec
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
                        srat.apic_to_domain.insert(id, domain);
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
                        // XXX: Technically this should be an underflow
                        let end = length.checked_sub(1)
                            .ok_or(Error::MemoryAffinityOverflow)?;
                        let end = start.checked_add(end)
                            .ok_or(Error::MemoryAffinityOverflow)?;

                        // XXX: And this should be a rangeset error
                        srat.domain_to_ranges.entry(domain)
                            .or_insert(RangeSet::new())
                            .insert(Range::new(start as u64, end as u64)
                                .unwrap())
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
                        srat.apic_to_domain.insert(id, domain);
                    }
                },
                _ => unimplemented!(),
            }
        }

        Ok(srat)
    }
}
