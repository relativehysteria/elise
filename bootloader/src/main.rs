#![no_std]
#![no_main]

use bootloader::{efi, println};
use serial::SerialDriver;

#[unsafe(no_mangle)]
fn efi_main(image_handle: efi::ImageHandle,
            system_table: *mut efi::SystemTable) -> efi::Status {
    // Initialize the serial driver
    {
        let driver = unsafe { SerialDriver::init() };
        let mut shared = bootloader::SHARED.serial.lock();
        *shared = Some(driver);
    }

    // Store the system table for global use by the bootloader
    efi::SYSTEM_TABLE.store(system_table, core::sync::atomic::Ordering::SeqCst);

    // Get the free memory map from UEFI
    let mem_map = efi::memory::get_memory_map()
        .expect("Coudln't acquire the memory map from UEFI");

    println!("{mem_map:?}");

    // TODO: somehow download a file over pxe

    panic!("Reached the end of bootloader execution");
}
