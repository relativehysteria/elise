//! Requirements for The Rust Core Library™.

#![no_std]
#![allow(missing_docs)]

use core::arch::asm;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcpy(dest: *mut u8, src: *mut u8, n: usize)
        -> *mut u8 {
    unsafe { memmove(dest, src, n) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memmove(dest: *mut u8, src: *mut u8, n: usize)
        -> *mut u8 {
    // If the `src` is placed before `dest`, copy the memory backwards.
    // Thus the memory won't overwrite itself as it copies bytes.
    if src < dest {
        let mut i = n;
        while i != 0 {
            i -= 1;
            unsafe { *dest.offset(i as isize) = *src.offset(i as isize); }
        }
    } else {
        let mut i = 0;
        while i < n {
            unsafe { *dest.offset(i as isize) = *src.offset(i as isize); }
            i += 1;
        }
    }
    dest
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcmp(s1: *mut u8, s2: *const u8, n: usize) -> i32 {
    let mut i = 0;
    while i < n {
        let a = unsafe { *s1.offset(i as isize) };
        let b = unsafe { *s2.offset(i as isize) };
        if a != b {
            return (a - b) as i32;
        }
        i += 1;
    }
    0
}

#[unsafe(no_mangle)]
#[cfg(target_arch = "x86_64")]
pub unsafe extern "C" fn memset(s: *const u8, c: i32, n: usize) -> *const u8 {
    if n == 0 { return s; }
    unsafe {
        asm!(
            "rep stosb",
            in("rax") c,
            inout("rdi") s => _,
            inout("rcx") n => _);
    }
    s
}

#[unsafe(no_mangle)]
#[cfg(target_arch = "x86")]
pub unsafe extern "C" fn memset(s: *const u8, c: i32, n: usize) -> *const u8 {
    if n == 0 { return s; }
    unsafe {
        asm!(
            "rep stosb",
            in("eax") c,
            inout("edi") s => _,
            inout("ecx") n => _);
    }
    s
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strlen(s: *const u8) -> usize {
    let mut i = 0;
    while unsafe { *s.offset(i as isize) } != b'\0' {
        i += 1;
    }
    i
}
