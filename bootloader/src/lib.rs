#![no_std]
#![feature(alloc_error_handler)]

pub extern crate alloc;

#[macro_use] pub mod print;
pub mod efi;
pub mod panic;
pub mod mm;
pub mod trampoline;

mod embedded;
pub use embedded::INITIAL_KERNEL_IMAGE;
pub use embedded::TRAMPOLINE;

/// Data shared between the bootloader and the kernel
pub static SHARED: shared_data::Shared = shared_data::Shared::new();
