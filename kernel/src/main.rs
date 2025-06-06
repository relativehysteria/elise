#![no_std]
#![no_main]

#[unsafe(export_name="_start")]
extern "sysv64" fn entry(shared: page_table::PhysAddr) -> ! {
    // This is the kernel entry point for all cores on the system

    // Initialize core locals
    kernel::core_locals::init(shared);

    // Disable interrupts just to match the `enable_interrupts()` call later
    unsafe { kernel::core!().disable_interrupts(); }

    // Initialize the interrupts
    kernel::interrupts::init();

    // Initialize the local APIC
    unsafe { kernel::apic::local::init(); }

    // BSP routines; one time initialization for the kernel
    if kernel::core!().is_bsp() {
        // Calibrate the TSC
        unsafe { kernel::time::calibrate(); }

        // Initialize PCI devices and drivers
        unsafe { kernel::pci::init(); }

        // Initialize NUMA information and bring up all APICs on the system
        unsafe { kernel::acpi::init().expect("Couldn't parse ACPI tables"); }

        // Enable the APIC timer. This timer is used to check the serial port
        // periodically to see if the user wants to issue a soft reboot
        unsafe {
            kernel::core!().apic().lock().as_mut().unwrap()
                .enable_reboot_timer();
        }
    }

    // The core is ready, enable interrupts!
    unsafe { kernel::core!().enable_interrupts(); }

    // Check in that this core has booted and is ready!
    kernel::apic::check_in();

    cpu::halt();
}
