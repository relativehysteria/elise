//! KERNEL STUFF

#![no_std]
#![forbid(missing_docs)]
#![feature(alloc_error_handler)]

pub mod core_locals;
pub mod mm;

/// Data shared between the bootloader and the kernel
pub static SHARED: oncelock::OnceLock<&'static shared_data::Shared> =
    oncelock::OnceLock::new();
