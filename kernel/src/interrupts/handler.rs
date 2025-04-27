//! Defined interrupt handlers

#![allow(unused_variables)]

use core::sync::atomic::Ordering;

use crate::interrupts::InterruptArgs;
use crate::panic::bsp_in_panic;
use crate::apic::{set_core_state, ApicState};

/// NMI handler
///
/// NMIs are used in the kernel to signalize and propagate panics. Whenever any
/// core causes a panic, it sends an NMI to the BSP, which then sends NMIs to
/// all cores on the system, causing them to halt.
pub unsafe fn nmi(args: InterruptArgs) -> bool {
    // NMIs are triggered by panics or soft reboot requests. Essentially, when
    // we get an NMI, we don't expect to recover

    // Don't recursively re-panic on the BSP
    if core!().is_bsp() && bsp_in_panic() {
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

/// Soft Reboot Timer handler
///
/// The soft reboot timer is an APIC timer that causes us to periodically check
/// the serial port to see if the user wants to issue a soft reboot
pub unsafe fn soft_reboot_timer(_args: InterruptArgs) -> bool {
    // Only allow soft reboot attempts from the BSP
    if !core!().is_bsp() { return true; }

    // Attempt to get a byte from the serial port
    let byte = { core!().shared.serial.lock().as_mut().unwrap().read_byte() };

    // TODO: I don't like the fact that these are hardcoded here

    // Halt request
    if let Some(b'H') = byte {
        panic!("Halt requested by the user");
    }

    // Soft reboot request. This is also hardcoded in panic.rs :(
    if let Some(b'S') = byte {
        // Mark the kernel as rebooting and panic
        core!().shared.rebooting.store(true, Ordering::SeqCst);
        panic!("Soft reboot requested by the user");
    }

    true
}
