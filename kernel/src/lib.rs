//! KERNEL STUFF

#![no_std]
#![allow(internal_features, incomplete_features)]
#![allow(clippy::missing_safety_doc, clippy::module_inception)]
#![feature(alloc_error_handler)]
#![feature(generic_const_exprs)]
#![feature(lang_items)]

pub extern crate core_reqs;
pub extern crate alloc;

#[macro_use] pub mod print;
#[macro_use] pub mod core_locals;
pub mod panic;
pub mod mm;
pub mod interrupts;
pub mod apic;
pub mod time;
pub mod pci;
pub mod acpi;
pub mod net;

#[lang = "eh_personality"]
fn eh_personality() {}
