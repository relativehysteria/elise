use core::panic::PanicInfo;

use cpu;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    cpu::halt();
}
