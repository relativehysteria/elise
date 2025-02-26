#![no_std]

#[macro_use] pub mod print;
pub mod efi;
pub mod panic;

/// Data shared between the bootloader and the kernel
pub static SHARED: shared_data::Shared = shared_data::Shared::new();
