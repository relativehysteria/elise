#![no_std]
#![no_main]
#![allow(internal_features)]
#![feature(lang_items)]

use core::panic::PanicInfo;
use shared_data::{KERNEL_SHARED_BASE, Shared};
use kernel::{SHARED, core_locals};

#[lang = "eh_personality"]
fn eh_personality() {}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    unsafe { cpu::halt(); }
}

/// Initialize the shared structure as static in the kernel
fn init_shared() {
    unsafe {
        SHARED.set(&*(KERNEL_SHARED_BASE as *const Shared));
    }
}

#[unsafe(export_name="_start")]
extern "C" fn entry(core_id: u32) -> ! {
    // One time kernel initialization
    if core_id == 0 {
        // Initialize the shared structure
        init_shared();
    }

    // Initialize core locals for this core
    core_locals::init(core_id);

    panic!();
}
