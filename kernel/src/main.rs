#![no_std]
#![no_main]

use core::sync::atomic::{AtomicU32, Ordering};

#[inline]
fn header(core_id: u32) {
    let header =
r#"
                   ┌────────────────────┐
───────────────────│ ENTERED THE KERNEL │───────────────────
                   └────────────────────┘     core:"#;
    kernel::println!("{header} {core_id:X}");
}

/// The cumulative variable used for allocating core IDs
static NEXT_CORE_ID: AtomicU32 = AtomicU32::new(0);

#[unsafe(export_name="_start")]
extern "sysv64" fn entry() -> ! {
    // This is the kernel entry point for all cores on the system

    // Initialize core locals
    kernel::core_locals::init(NEXT_CORE_ID.fetch_add(1, Ordering::SeqCst));

    // Print the kernel header
    header(kernel::core!().id);

    // Initialize the interrupts
    kernel::interrupts::init();

    // Initialize the APIC
    unsafe { kernel::apic::init(); }

    // BSP routines; one time initialization for the kernel
    if kernel::core!().id == 0 {
        // Calibrate the TSC
        unsafe { kernel::time::calibrate(); }

        // Initialize PCI devices and drivers
        unsafe { kernel::pci::init(); }

        // Initialize NUMA information and bring up all APICs on the system
        unsafe { kernel::acpi::init().expect("Couldn't parse ACPI tables"); }
    }

   // Check in that this core has booted and is ready!
    kernel::acpi::apic::check_in();

    panic!();
}
