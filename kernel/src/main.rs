#![no_std]
#![no_main]

#[unsafe(export_name="_start")]
extern "sysv64" fn entry(shared: page_table::PhysAddr) -> ! {
    // This is the kernel entry point for all cores on the system

    // Initialize core locals
    kernel::core_locals::init(shared);

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
