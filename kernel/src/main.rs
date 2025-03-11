#![no_std]
#![no_main]
#![allow(internal_features)]
#![feature(lang_items)]

use page_table::PhysAddr;
use core::panic::PanicInfo;

#[lang = "eh_personality"]
fn eh_personality() {}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    unsafe { cpu::halt(); }
}

#[unsafe(export_name="_start")]
extern "C" fn entry(shared: PhysAddr, core_id: u32) -> ! {
    panic!();
}
