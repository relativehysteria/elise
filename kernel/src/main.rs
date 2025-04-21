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

    // Check in that this core has booted and is ready!
    kernel::acpi::apic::check_in();

    //loop { unsafe { core::arch::asm!("hlt"); } }
    panic!();
}
