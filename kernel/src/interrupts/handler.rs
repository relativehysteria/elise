//! Defined interrupt handlers

#![allow(unused_variables)]

use crate::interrupts::InterruptArgs;
use crate::panic::in_panic;
use crate::acpi::apic::{set_core_state, ApicState};

/// NMI handler.
pub unsafe fn nmi(args: InterruptArgs) -> bool {
    // NMIs are triggered by panics or soft reboot requests. Essentially, when
    // we get an NMI, we don't expect to recover

    // Don't recursively re-panic on the BSP
    if core!().is_bsp() && in_panic() {
        return true;
    }

    // If this is the BSP, some other core has panicked
    if core!().is_bsp() {
        panic!("Panic occured on another core");
    } else {
        unsafe {
            // Set that we're halted and halt forever
            set_core_state(core!().apic_id().unwrap(), ApicState::Halted);
            cpu::halt();
        }
    }
}

/// Page Fault handler
pub unsafe fn page_fault(args: InterruptArgs) -> bool {
    false
}
