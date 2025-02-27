#![no_std]
#![feature(alloc_error_handler)]

pub extern crate alloc;

#[macro_use] pub mod print;
pub mod efi;
pub mod panic;
pub mod mm;

/// Data shared between the bootloader and the kernel
pub static SHARED: shared_data::Shared = shared_data::Shared::new();
