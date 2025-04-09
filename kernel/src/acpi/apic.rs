//! Routines and structures for manipulating NUMA topologies and APIC states

use core::sync::atomic::{ Ordering, AtomicU32, AtomicU8 };
use alloc::vec::Vec;
use alloc::collections::BTreeMap;

use rangeset::RangeSet;
use crate::acpi::Error;

/// Maximum number of cores supported by the system.
///
/// This value can be technically arbitrarily large. However, large values will
/// cause the global APIC/core tracking variables to grow large as well.
pub const MAX_CORES: usize = 1024;

/// Map of APIC IDs to their memory domains.
///
/// APIC ID is the index and the value is the domain ID.
pub static APIC_TO_MEM_DOMAIN: [AtomicU32; MAX_CORES] =
    [const { AtomicU32::new(0) }; MAX_CORES];

/// Map of the APIC IDs to their APIC states.
static APIC_STATES: [AtomicU8; MAX_CORES] =
    [const { AtomicU8::new(ApicState::None as u8) }; MAX_CORES];

/// APIC to memory domain mapping
pub type ApicDomains = BTreeMap<u32, u32>;

/// Memory domain to physical memory ranges mapping
pub type MemoryDomains = BTreeMap<u32, RangeSet>;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
/// The possible states of APICs
pub enum ApicState {
    /// The core is registered in the kernel and is running
    Online = 1,

    /// The core has been launched but is yet to run within the kernel
    Launched = 2,

    /// The core is present but has not yet been launched
    Offline = 3,

    /// This APIC ID does not exist
    None = 4,

    /// This APIC ID has disabled interrupts and is halted forever
    Halted = 5,
}

/// Initialize the APIC and NUMA mappings and register them with the memory
/// manager
pub fn init(
    apics: Option<Vec<u32>>,
    apic_domains: Option<ApicDomains>,
    mem_domains: Option<MemoryDomains>,
) -> Result<(), Error> {
    // Register the APIC IDs to their memory domains and notify the memory
    // manager about the NUMA mappings
    if let (Some(ad), Some(_md)) = (apic_domains, mem_domains) {
        ad.iter().for_each(|(&apic, &domain)| {
            APIC_TO_MEM_DOMAIN[apic as usize]
                .store(domain, Ordering::Relaxed);
        });

        // TODO: Notify the memory manager about the NUMA mappings
    }

    // Initialize the state of all functional APICs
    if let Some(apics) = &apics {
        apics.iter().for_each(|&apic_id| {
            APIC_STATES[apic_id as usize]
                .store(ApicState::Offline as u8, Ordering::SeqCst)
        })
    }

    Ok(())
}
