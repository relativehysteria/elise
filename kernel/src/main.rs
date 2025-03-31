#![no_std]
#![no_main]

extern crate alloc;
use kernel::{core_locals, interrupts};

#[unsafe(export_name="_start")]
extern "C" fn entry(core_id: u32) -> ! {
    // Initialize core locals for this core
    core_locals::init(core_id);

    let a: alloc::vec::Vec<u8> = alloc::vec::Vec::with_capacity(4097);

    panic!();
}
