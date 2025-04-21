//! Routines and structures for manipulating NUMA topologies and APIC states

use core::sync::atomic::{Ordering, AtomicU32, AtomicU8};
use alloc::vec::Vec;
use alloc::collections::BTreeMap;
use page_table::PhysAddr;
use rangeset::RangeSet;
use shared_data::BootloaderState;
use crate::acpi::Error;
use crate::mm::slice_phys_mut;

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

/// The total amount of cores on the system
static TOTAL_CORES: AtomicU32 = AtomicU32::new(0);

/// The real mode code all APs start their execution at
static ENTRY_CODE: &'static [u8] =
    include_bytes!("../../target/apic_entry.bin");

/// The real mode address where `AP_ENTRY_CODE` will be mapped. This value is
/// based on the first `[org n]` in the `apic_entry.asm` file
const ENTRY_ADDR: u16 = 0x8000;

/// Check in that the current core has booted and wait for the rest of the cores
pub fn check_in() {
    /// Number of cores which have checked in
    static CORES_CHECKED_IN: AtomicU32 = AtomicU32::new(0);

    // Transition from launched to online
    let old_state = APIC_STATES[unsafe { core!().apic_id().unwrap() as usize }]
        .compare_exchange(ApicState::Launched as u8,
                          ApicState::Online   as u8,
                          Ordering::SeqCst,
                          Ordering::SeqCst).unwrap_or_else(|x| x);

    if core!().is_bsp() {
        // BSP should already be marked online
        assert!(old_state == ApicState::Online as u8,
                "BSP not marked online in APIC state");
    } else {
        // Make sure that we only ever go from launched to online, any other
        // transition is invalid
        assert!(old_state == ApicState::Launched as u8,
                "Invalid core state transition");
    }

    // Check in!
    CORES_CHECKED_IN.fetch_add(1, Ordering::SeqCst);

    // Get the total number of cores on the system
    let num_cores = TOTAL_CORES.load(Ordering::SeqCst);
    assert!(num_cores != 0, "Called `check_in()` before ACPI was parsed!");

    // Wait for all cores to be checked in
    while CORES_CHECKED_IN.load(Ordering::SeqCst) != num_cores {
        core::hint::spin_loop();
    }
}

/// Set the current execution state of a given APIC ID
pub unsafe fn set_core_state(id: u32, state: ApicState) {
    APIC_STATES[id as usize].store(state as u8, Ordering::SeqCst);
}

/// Get the APIC state of a given APIC ID
pub fn core_state(id: u32) -> ApicState {
    APIC_STATES[id as usize].load(Ordering::SeqCst).into()
}

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

impl From<u8> for ApicState {
    /// Convert a raw `u8` into an `ApicState`
    fn from(val: u8) -> ApicState {
        match val {
            1 => ApicState::Online,
            2 => ApicState::Launched,
            3 => ApicState::Offline,
            4 => ApicState::None,
            5 => ApicState::Halted,
            _ => panic!("Invalid ApicState from `u8`"),
        }
    }
}

/// Initialize the NUMA mappings and register them with the memory manager,
/// and also initialize and bring up the other cores on the system
pub fn init(
    apics: Option<Vec<u32>>,
    apic_domains: Option<ApicDomains>,
    mem_domains: Option<MemoryDomains>,
) -> Result<(), Error> {
    // Register the APIC IDs to their memory domains and notify the memory
    // manager about the NUMA mappings
    if let (Some(ad), Some(md)) = (apic_domains, mem_domains) {
        ad.iter().for_each(|(&apic, &domain)| {
            APIC_TO_MEM_DOMAIN[apic as usize]
                .store(domain, Ordering::Relaxed);
        });

        // Inform the memory allocator of our NUMA mappings
        unsafe { crate::mm::register_numa(ad, md); }
    }

    // Initialize the state of all functional APICs
    if let Some(apics) = &apics {
        apics.iter().for_each(|&apic_id| {
            APIC_STATES[apic_id as usize]
                .store(ApicState::Offline as u8, Ordering::SeqCst)
        })
    }

    // Mark the current core as online
    let cur_id = unsafe { core!().apic_id().unwrap() };
    unsafe { set_core_state(cur_id, ApicState::Online); }

    // Save the total number of cores on the system. This will be used during
    // the `check_in()` loop to wait for all cores to come online.
    TOTAL_CORES.store(
        apics.as_ref().map(|x| x.len() as u32).unwrap_or(1),
        Ordering::SeqCst);

    // Map in the AP entry code
    let code_len = ENTRY_CODE.len();
    let addr = PhysAddr(ENTRY_ADDR as u64);
    let to_fill_in = slice_phys_mut(addr, code_len as u64);
    to_fill_in.copy_from_slice(ENTRY_CODE);

    // Fill in the bootloader state to the end of the code
    let bstate_size = core::mem::size_of::<BootloaderState>();
    let bstate = core!().shared.bootloader().get();
    let bstate_fill_in = &mut to_fill_in[code_len - bstate_size..];
    unsafe {
        core::ptr::copy_nonoverlapping(
            bstate as *const BootloaderState as *const u8,
            bstate_fill_in.as_mut_ptr(),
            bstate_size,
        );
    }

    // Launch all other cores
    if let Some(apics) = apics {
        // Acquire exclusive access to the APIC for this core
        let mut apic = unsafe { core!().apic().lock() };
        let apic = apic.as_mut().unwrap();

        // Go through all APICs on the system, skipping this core
        for &id in apics.iter().filter(|&&id| id != cur_id) {
            // Mark the core as launched
            unsafe { set_core_state(id, ApicState::Launched); }

            // INIT-SIPI-SIPI; launch the core
            let entry = (ENTRY_ADDR / 0x1000) as u32;
            unsafe {
                apic.ipi(id, 0x4500);
                apic.ipi(id, 0x4600 + entry);
                apic.ipi(id, 0x4600 + entry);
            }

            // Wait for the core to come online
            while core_state(id) != ApicState::Online {
                crate::time::sleep(1_000);
                core::hint::spin_loop();
            }
        }
    }

    Ok(())
}
