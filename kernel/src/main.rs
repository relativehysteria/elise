#![no_std]
#![no_main]

use kernel::{core_locals, interrupts};

#[unsafe(export_name="_start")]
extern "sysv64" fn entry(core_id: u32) -> ! {
    // Initialize core locals for this core
    core_locals::init(core_id);

    // Initialize the interrupts for this core
    interrupts::init();

    unsafe {
        core::ptr::write_volatile(0x100134 as *mut u8, 123);
    }

    panic!();
}
