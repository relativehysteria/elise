#![no_std]
#![no_main]

use kernel::{core_locals, interrupts};

#[unsafe(export_name="_start")]
extern "C" fn entry(core_id: u32) -> ! {
    // Initialize core locals for this core
    core_locals::init(core_id);

    // Initialize the interrupts for this core
    interrupts::init();

    panic!();
}
