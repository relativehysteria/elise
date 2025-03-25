//! Kernel panic handler
use core::panic::PanicInfo;

#[panic_handler]
/// This is the panic routine used by rust within our kernel
pub fn panic(_info: &PanicInfo) -> ! {
    // Halt
    unsafe { cpu::halt() };
}
