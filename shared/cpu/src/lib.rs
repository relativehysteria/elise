//! Arch specific routines that interface with the CPU directly

#![no_std]

use core::arch::asm;

#[inline]
/// Clears interrupts and halts the core
pub fn halt() -> ! {
    unsafe { asm!("cli", "hlt"); }
    loop { core::hint::spin_loop(); }
}
