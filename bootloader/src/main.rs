#![no_std]
#![no_main]

use bootloader::efi;
use serial::SerialDriver;

#[unsafe(no_mangle)]
fn efi_main(image_handle: efi::BootloaderImagePtr,
            system_table: efi::SystemTablePtr) -> efi::Status {
    // Initialize the serial driver
    {
        let driver = unsafe { SerialDriver::init() };
        let mut shared = bootloader::SHARED.serial.lock();
        *shared = Some(driver);
    }

    // Initialize the EFI structures required for the bootloader to work
    efi::init_efi(image_handle, system_table);

    // Test out our PXE code
    unsafe { efi::pxe::download("test") };

    panic!("Reached the end of bootloader execution");
}
