#![no_std]
#![no_main]

use bootloader::{efi, mm};
use serial::SerialDriver;

#[unsafe(no_mangle)]
extern "C" fn efi_main(image_handle: efi::BootloaderImagePtr,
            system_table: efi::SystemTablePtr) -> efi::Status {
    // Initialize the serial driver
    {
        let driver = unsafe { SerialDriver::init() };
        let mut shared = bootloader::SHARED.serial.lock();
        *shared = Some(driver);
    }

    // Get the memory map from UEFI and exit the boot services
    let map = unsafe { efi::memory_map_exit(system_table, image_handle) };

    // Initialize the memory manager
    mm::init(map.expect("Couldn't acquire memory map from UEFI."));

    panic!("Reached the end of bootloader execution");
}
