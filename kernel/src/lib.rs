//! KERNEL STUFF

#![no_std]
#![feature(alloc_error_handler)]

#![feature(lang_items)]
#![allow(internal_features)]

pub extern crate core_reqs;
pub extern crate alloc;

#[macro_use] pub mod print;
#[macro_use] pub mod core_locals;
pub mod panic;
pub mod mm;
pub mod interrupts;
pub mod apic;

#[lang = "eh_personality"]
fn eh_personality() {}
