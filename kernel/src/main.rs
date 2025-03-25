#![no_std]
#![no_main]
#![allow(internal_features)]
#![feature(lang_items)]

use core::panic::PanicInfo;
use kernel::core_locals;

#[lang = "eh_personality"]
fn eh_personality() {}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    unsafe { cpu::halt(); }
}

#[unsafe(export_name="_start")]
extern "C" fn entry(core_id: u32) -> ! {
    // Initialize core locals for this core
    core_locals::init(core_id);

    panic!();
}
