#![no_std]
#![no_main]

#[inline]
fn header() {
    let header =
r#"
                   ┌────────────────────┐
───────────────────│ ENTERED THE KERNEL │───────────────────
                   └────────────────────┘
"#;
    kernel::print!("{header}");
}

#[unsafe(export_name="_start")]
extern "sysv64" fn entry(core_id: u32) -> ! {
    // This is the kernel entry point for all cores on the system

    // Initialize core locals
    kernel::core_locals::init(core_id);

    // Print the kernel header
    if kernel::core!().id == 0 { header() }

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

    panic!();
}
