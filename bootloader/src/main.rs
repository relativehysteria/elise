#![no_std]
#![no_main]

use bootloader::{efi, print};
use serial::SerialDriver;
use spinlock::SpinLock;
use shared_data::Shared;

#[unsafe(no_mangle)]
fn efi_main(image_handle: efi::ImageHandle,
            system_table: *mut efi::SystemTable) -> efi::Status {
    // Initialize the serial driver
    {
        let driver = unsafe { SerialDriver::init() };
        let mut shared = bootloader::SHARED.serial.lock();
        *shared = Some(driver);
    }

    panic!("Reached the end of bootloader execution");
}
