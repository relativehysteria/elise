//! MADT implementation

use alloc::vec::Vec;
use alloc::collections::BTreeMap;
use crate::apic::ioapic::IoApic;
use crate::acpi::{SdtHeader, Error, Table, ENABLED, parse_table_entries};

/// Flag showing that an APIC is online capable
const ONLINE_CAPABLE: u32 = 1 << 1;

/// Information returned when parsing the MADT table
pub struct Madt {
    /// ID vector of all usable APICs
    pub apics: Vec<u32>,

    /// Vector of all IO APICs
    pub io_apics: Vec<IoApic>,

    /// Vector of all ISA source overrides
    pub isa_overrides: BTreeMap<u8, u32>,
}

impl Madt {
    pub unsafe fn parse(hdr_ptr: *const SdtHeader) -> Result<Self, Error> {
        // Get the entries from the table
        let entries = unsafe {
            parse_table_entries(hdr_ptr, 2 * size_of::<u32>())?
        };

        // Create the info struct that will be returned
        let mut madt = Madt {
            apics: Vec::new(),
            io_apics: Vec::new(),
            isa_overrides: BTreeMap::new(),
        };

        // The error we will return if the entry's length doesn't match the expected
        let mismatch_err = Err(Error::SizeMismatch(Table::Madt));

        // Go through each entry and save the IDs of functional APICs
        for entry in entries {
            match entry.typ {
                // Local APIC
                0 => {
                    // Validate the length
                    if entry.len != 8 { return mismatch_err; }

                    // Read the APIC ID and flags
                    let id    = entry.read::<u8>(3) as u32;
                    let flags = entry.read::<u32>(4);

                    // If the CPU is enabled, or can be enabled, save the ID
                    if (flags & ENABLED) != 0 || (flags & ONLINE_CAPABLE) != 0 {
                        madt.apics.push(id);
                    }
                },
                // IO APIC
                1 => {
                    // Validate the length
                    if entry.len != 12 { return mismatch_err; }

                    // Read the fields
                    let id   = entry.read::<u8>(2);
                    let addr = entry.read::<u32>(4);
                    let gsi  = entry.read::<u32>(8);

                    // Save the struct
                    madt.io_apics.push(IoApic::new(id, addr, gsi));
                },
                // // Interrupt Source Override
                // 2 => {
                //     // Validate the length
                //     if entry.len != 10 { return mismatch_err; }

                //     // Read the source int, dest int and the flags
                //     let source = entry.read::<u8>(3);
                //     let gsi    = entry.read::<u32>(4);
                //     let flags  = entry.read::<u16>(8);

                //     // Only support interrupts that conform to the bus spec
                //     // TODO: HANDLE FLAGS
                //     if flags != 0 { return Err(Error::UnhandledFlags); }

                //     // Insert the override and make sure this entry is unique
                //     if let Some(orig) = madt.isa_overrides.insert(source, gsi) {
                //         if orig != gsi {
                //             panic!("Multiple GSIs specified for ISA override.");
                //         }
                //     }
                // },
                // Local x2APIC
                9 => {
                    // Validate the length
                    if entry.len != 16 { return mismatch_err; }

                    // Read the APIC ID and flags
                    let id    = entry.read::<u32>(4);
                    let flags = entry.read::<u32>(8);

                    // If the CPU is enabled, or can be enabled, save the ID
                    if (flags & ENABLED) != 0 || (flags & ONLINE_CAPABLE) != 0 {
                        madt.apics.push(id);
                    }
                },
                _ => {},
            }
        }

        Ok(madt)
    }
}
