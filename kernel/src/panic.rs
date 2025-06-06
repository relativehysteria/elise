//! Kernel panic handler and soft reboot routines

use core::sync::atomic::{AtomicPtr, AtomicBool, Ordering};
use core::panic::PanicInfo;

use crate::apic::{ApicState, core_state, MAX_APIC_ID, LocalApic};

/// Tracks whether we're currently in the process of a panic on the BSP
static BSP_IN_PANIC: AtomicBool = AtomicBool::new(false);

/// Pointer to a pending panic. When a non-BSP core panics, it will
/// place its `PanicInfo` pointer into here, NMI the core 0, and then halt
/// forever.
static PANIC_PENDING: AtomicPtr<PanicInfo> =
    AtomicPtr::new(core::ptr::null_mut());

/// Returns whether we're currently in the process of a panic on the BSP
#[inline]
pub fn bsp_in_panic() -> bool {
    BSP_IN_PANIC.load(Ordering::SeqCst)
}

/// The NMI ICR
const NMI: u32 = (1 << 14) | (4 << 8);
// TODO: encode the ICRs somehow. I don't like this^ nor 0x4500 for INIT

/// This is the panic routine used by rust within our kernel
#[panic_handler]
pub fn panic(info: &PanicInfo) -> ! {
    // Disable interrupts, we're not gonna recover
    unsafe { core!().disable_interrupts(); }

    // If this is not the BSP, notify it of our panic, and halt this core
    if !core!().is_bsp() {
        // Make sure there's only ever one pending panic
        let no_panic_pending = PANIC_PENDING.compare_exchange(
            core::ptr::null_mut(),
            info as *const _ as *mut _,
            Ordering::SeqCst,
            Ordering::SeqCst).is_ok();

        // If the BSP isn't yet panicking and there's no other pending panic,
        // send out an NMI to the BSP, telling it there's a pending panic now
        if !bsp_in_panic() && no_panic_pending {
            // Notify the BSP of our panic via NMI
            unsafe {
                // Get access to the APIC
                let apic = &mut *core!().apic().shatter();
                let apic = apic.as_mut().unwrap();

                // Send out the NMI
                apic.ipi(0, NMI);
            }
        }

        // Halt the core forever
        cpu::halt();
    }

    // At this point, we know that the BSP has panicked. Sent an NMI to all
    // other cores on the system and wait for them to halt
    BSP_IN_PANIC.store(true, Ordering::SeqCst);

    // Save the panic information
    let our_info: *const PanicInfo = info;
    let other_info: *const PanicInfo = PANIC_PENDING.load(Ordering::SeqCst);

    // Print information about the panic
    for &(bsp_msg, info) in &[
        ("non-BSP", other_info),
        ("BSP", our_info),
    ] {
        // Only print if there is panic info
        if info.is_null() { continue; }

        let info = unsafe { &*info };

        // Print the location info
        if let Some(loc) = info.location() {
            print_shatter!("\n!!! {} PANIC !!! {} {}:{} ----",
                bsp_msg, loc.file(), loc.line(), loc.column());
        }

        // Print the message
        println_shatter!(" {} ----\n", info.message());
    }

    // Disable all other cores and wait for them to halt
    let apic = unsafe {
        // Get access to the APIC
        let apic = &mut *core!().apic().shatter();
        let apic = apic.as_mut().unwrap();

        // Disable the cores
        disable_cores(apic);
        apic
    };

    // Wait for a soft reboot request to be issued
    {
        let mut serial = core!().shared.serial.lock();
        let serial = serial.as_mut().unwrap();
        while !core!().shared.rebooting.load(Ordering::SeqCst) {
            if serial.read_byte() == Some(b'S') {
                core!().shared.rebooting.store(true, Ordering::SeqCst);
            }
        }
    }

    // Soft reboot the system
    unsafe { soft_reboot(apic); }
}

/// Disable all non-BSP cores on the system
unsafe fn disable_cores(apic: &mut LocalApic) {
    // We don't allow disabling other cores on non-BSP cores
    assert!(core!().is_bsp(), "Attempted to disable other cores on non-BSP");

    // Send out NMIs to all non-BSP cores and wait for them to halt
    if let Some(bsp_id) = core!().apic_id() {
        // Only shut down the other APICs if they were initialized
        if !MAX_APIC_ID.initialized() { return; }

        for id in 0..*MAX_APIC_ID.get() {
            // Don't NMI the BSP
            if id == bsp_id { continue; }

            // If this APIC is online, send an NMI and wait for it to halt
            let state = core_state(id);
            if state == ApicState::Online {
                // Send the NMI and wait for the core to halt
                while core_state(id) != ApicState::Halted {
                    unsafe { apic.ipi(id, NMI); }
                    crate::time::sleep(1_000);
                    core::hint::spin_loop();
                }

                // INIT the core
                unsafe { apic.ipi(id, 0x4500); }
                crate::time::sleep(1_000);
            }
        }
    }
}

/// Halt and INIT all processors, put everything into a predictable state, shut
/// down the kernel and perform a software reboot
pub unsafe fn soft_reboot(apic: &mut LocalApic) -> ! {
    // Mark the kernel as rebooting
    core!().shared.rebooting.store(true, Ordering::SeqCst);

    // Disable other cores
    unsafe { disable_cores(apic); }

    // Reset all PCI devices
    unsafe { crate::pci::reset_devices(); }

    // Reset the APIC
    unsafe { apic.reset(); }

    // Get the trampoline pointer
    let tramp = unsafe { shared_data::get_trampoline() };

    // Bootloader keeps its own pointer to the shared struct, we don't need to
    // set it
    let shared = page_table::PhysAddr(0);

    // Get the bootloader state
    let bstate = core!().shared.bootloader().get();

    // Jump to the bootloader
    unsafe {
        tramp(bstate.entry, bstate.stack, bstate.page_table.clone(), shared)
    };
}
