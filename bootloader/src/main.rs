#![no_std]
#![no_main]

use bootloader::efi;
use serial::SerialDriver;

#[unsafe(no_mangle)]
fn efi_main(image_handle: efi::ImageHandle,
            system_table: *mut efi::SystemTable) -> efi::Status {

    // Initialize the serial driver
    let mut serial = unsafe { SerialDriver::init() };

    serial.write(b"hello world!\n");

    panic!();
}
