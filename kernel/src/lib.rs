//! KERNEL STUFF

#![no_std]
#![forbid(missing_docs)]
#![feature(alloc_error_handler)]

pub extern crate core_reqs;
pub extern crate alloc;

#[macro_use] pub mod print;
#[macro_use] pub mod core_locals;
pub mod mm;
