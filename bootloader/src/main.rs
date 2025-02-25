#![no_std]
#![no_main]

use bootloader::{efi, print};
use serial::SerialDriver;
use spinlock::SpinLock;

#[unsafe(no_mangle)]
fn efi_main(image_handle: efi::ImageHandle,
            system_table: *mut efi::SystemTable) -> efi::Status {

    // Initialize the serial driver
    {
        let driver = unsafe { SerialDriver::init() };
        let mut global_driver = print::SERIAL_DRIVER.lock();
        *global_driver = Some(driver);
    }

    print!("Hello world!\n");

    panic!();
}
