#![no_std]
#![no_main]

use kernel::core_locals;

#[unsafe(export_name="_start")]
extern "C" fn entry(core_id: u32) -> ! {
    // Initialize core locals for this core
    core_locals::init(core_id);

    kernel::println!("heyo");

    panic!();
}
