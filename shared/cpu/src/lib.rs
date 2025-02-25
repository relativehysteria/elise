//! Arch specific routines that interface with the CPU directly

#![no_std]

use core::arch::asm;

#[inline]
/// Clears interrupts and halts the core
pub fn halt() -> ! {
    unsafe { asm!("cli", "hlt"); }
    loop { core::hint::spin_loop(); }
}

#[inline]
/// Read a byte from I/O port `addr`
pub unsafe fn in8(addr: *const u8) -> u8 {
    let mut byte: u8;
    unsafe { asm!("in al, dx", in("dx") addr, out("al") byte) };
    byte
}

#[inline]
/// Write a `byte` to I/O port `addr`
pub unsafe fn out8(addr: *const u8, byte: u8) {
    unsafe { asm!("out dx, al", in("dx") addr, in("al") byte) };
}

#[inline]
/// Read bytes from I/O port `addr`
pub unsafe fn in32(addr: *const u32) -> u32 {
    let mut bytes: u32;
    unsafe { asm!("in dx, eax", in("dx") addr, out("eax") bytes) };
    bytes
}

#[inline]
/// Write `bytes` to I/O port `addr`
pub unsafe fn out32(addr: *const u32, bytes: u32) {
    unsafe { asm!("out dx, eax", in("dx") addr, in("eax") bytes) };
}

#[inline]
/// Read a value from the Model-Specific Register `msr`
pub unsafe fn rdmsr(msr: u32) -> u64 {
    let high: u32;
    let low: u32;
    unsafe { asm!("rdmsr", in("ecx") msr, out("edx") high, out("eax") low) };
    ((high as u64) << 32) | (low as u64)
}

#[inline]
/// Write a 64-bit `val` to the Model-Specific Register `msr`
pub unsafe fn wrmsr(msr: u32, val: u64) {
    let high = (val >> 32) as u32;
    let low = val as u32;
    unsafe { asm!("wrmsr", in("ecx") msr, in("edx") high, in("eax") low) };
}

#[inline]
/// Calls RDTSC
pub unsafe fn rdtsc() -> usize {
    unsafe { core::arch::x86_64::_rdtsc() as usize }
}
