#![no_std]
#![no_main]

#[unsafe(export_name="_start")]
extern "sysv64" fn entry(core_id: u32) -> ! {
    // This is the kernel entry point for all cores on the system

    // Initialize core locals
    kernel::core_locals::init(core_id);

    // Initialize the interrupts
    kernel::interrupts::init();

    // Initialize the APIC
    unsafe { kernel::apic::init(); }

    // BSP routines
    if kernel::core!().id == 0 {
        // Calibrate the TSC
        unsafe { kernel::time::calibrate() };
    }

    panic!();
}
