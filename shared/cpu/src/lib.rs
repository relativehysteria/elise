//! Arch specific routines that interface with the CPU directly

#![no_std]

mod features;
pub use features::*;

use core::arch::asm;

#[inline]
/// Halts the core in a loop forever
pub unsafe fn halt() -> ! {
    loop {
        unsafe { asm!("hlt"); }
        core::hint::spin_loop();
    }
}

#[inline]
/// Disables the interrupts on this core
pub unsafe fn disable_interrupts() {
    unsafe { asm!("cli"); }
}

#[inline]
/// Disables the interrupts on this core
pub unsafe fn enable_interrupts() {
    unsafe { asm!("sti"); }
}

#[inline]
/// Read a byte from I/O port `addr`
pub unsafe fn in8(addr: u16) -> u8 {
    let mut byte: u8;
    unsafe { asm!("in al, dx", in("dx") addr, out("al") byte) };
    byte
}

#[inline]
/// Write a `byte` to I/O port `addr`
pub unsafe fn out8(addr: u16, byte: u8) {
    unsafe { asm!("out dx, al", in("dx") addr, in("al") byte) };
}

#[inline]
/// Read bytes from I/O port `addr`
pub unsafe fn in32(addr: u16) -> u32 {
    let mut bytes: u32;
    unsafe { asm!("in eax, dx", in("dx") addr, out("eax") bytes) };
    bytes
}

#[inline]
/// Write `bytes` to I/O port `addr`
pub unsafe fn out32(addr: u16, bytes: u32) {
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
/// Set the GS base
pub unsafe fn set_gs_base(base: u64) {
    const IA32_GS_BASE: u32 = 0xC0000101;
    unsafe { wrmsr(IA32_GS_BASE, base) };
}

#[inline]
/// Calls RDTSC
pub unsafe fn rdtsc() -> u64 {
    unsafe { core::arch::x86_64::_rdtsc() as u64 }
}

#[inline]
/// Canonicalizes the `addr`, making sure the highest `high_bits` are the same.
pub const fn canonicalize_address(high_bits: usize, addr: u64) -> u64 {
    assert!(high_bits < 64);
    (((addr as i64) << high_bits) >> high_bits) as u64
}

#[inline]
/// Read `cr2`
pub fn read_cr2() -> u64 {
    let mut cr2: u64;
    unsafe { asm!("mov {}, cr2", out(reg) cr2); }
    cr2
}

/// Performs cpuid passing in eax and ecx as parameters. Returns a tuple
/// containing the resulting (eax, ebx, ecx, edx)
#[inline]
pub unsafe fn cpuid(eax: u32, ecx: u32) -> (u32, u32, u32, u32) {
    let mut oeax: u32;
    let mut oebx: u32;
    let mut oecx: u32;
    let mut oedx: u32;

    unsafe {
        asm!(
            "push rbx",
            "cpuid",
            "mov {0:e}, ebx",
            "pop rbx",
            out(reg) oebx,
            out("edx") oedx,
            inout("eax") eax => oeax,
            inout("ecx") ecx => oecx,
        );
    }

    (oeax, oebx, oecx, oedx)
}
