//! Kernel panic handler and soft reboot routines

use core::panic::PanicInfo;

#[panic_handler]
/// This is the panic routine used by rust within our kernel
pub fn panic(info: &PanicInfo) -> ! {
    // Print the location info
    if let Some(loc) = info.location() {
       print_shatter!("\n!!! PANIC !!! {} {}:{} ----",
            loc.file(), loc.line(), loc.column());
    }

    // Print the message
    print_shatter!(" {} ----\n", info.message());

    // Halt
    unsafe { cpu::halt() };
}

pub unsafe fn soft_reboot() -> ! {
    // Get the trampoline pointer
    let tramp = unsafe { shared_data::get_trampoline() };

    let shared = core!().shared as *const shared_data::Shared;

    // Get the bootloader state and jump to the bootloader
    let bstate = core!().shared.bootloader().get();

    unsafe {
        tramp(bstate.entry, bstate.stack, bstate.page_table.clone(), shared)
    };
}
