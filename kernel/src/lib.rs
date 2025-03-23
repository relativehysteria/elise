#![no_std]

pub mod core_locals;
pub mod mm;

/// Data shared between the bootloader and the kernel
pub static SHARED: oncelock::OnceLock<&'static shared_data::Shared> =
    oncelock::OnceLock::new();
