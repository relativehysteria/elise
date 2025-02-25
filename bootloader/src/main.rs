#![no_std]
#![no_main]

use bootloader::efi;

#[unsafe(no_mangle)]
fn efi_main(image_handle: efi::ImageHandle,
            system_table: *mut efi::SystemTable) -> efi::Status {
    panic!();
}
