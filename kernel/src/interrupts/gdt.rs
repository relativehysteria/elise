//! The kernel GDT and related routines.
//!
//! Because the BSP first gets execution in long mode (because of UEFI), the
//! segment selectors and GDT are already set for it. However because we use a
//! critical stack in critical interrupts, we have to set up a TSS and load our
//! own GDT and update the selectors if needed. Unfortunately, there's no clean
//! way of changing segment registers in long mode, be it due to rust (which
//! doesn't allow long jumps, nor does it emit correct code with workarounds) or
//! due to architecture requirements (AMD doesn't allow changing selectors
//! without a mode change).
//!
//! For this reason, we do things in reverse; instead of loading a GDT and
//! changing the selectors, we create a GDT based on what's already in the
//! selectors and then load it.
//!
//! This is only required for the BSP, because we bring the APs to long mode
//! ourselves and therefore control the registers, but we'll use the same GDT
//! for all cores, so effectively, UEFI dictates the shape of our GDT in all
//! cores.

use alloc::boxed::Box;
use alloc::vec::Vec;
use alloc::vec;
use core::mem::ManuallyDrop;

/// A 64-bit task state segment structure
#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
pub struct Tss {
    reserved1:   u32,
    rsp:         [u64; 3],
    reserved2:   u64,
    ist:         [u64; 7],
    reserved3:   u64,
    reserved4:   u16,
    iopb_offset: u16,
}

#[repr(transparent)]
/// The kernel GDT
pub struct Gdt {
    /// The raw GDT entry
    pub raw: Vec<u64>,
}

impl Gdt {
    /// Create a new GDT, placing the long mode descriptors to where the CS and
    /// DS currently point (such that we don't have to change them when loading
    /// this GDT) and append a TSS with a critical stack at IST[0].
    ///
    /// Returns the GDT, the TSS and the offset of the TSS into GDT.
    pub fn new() -> (Self, Box<Tss>, u16) {
        // Get the selector indices
        let (cs_idx, ds_idx) = get_selector_indices();
        assert!(cs_idx != ds_idx, "CS and DS can't be the same value!");
        assert!(cs_idx != 0, "CS and DS can't be at index 0 in the GDT!");

        // This is the index where we'll insert the rest of the selectors
        let insert_idx = (cs_idx.max(ds_idx) + 1) as usize;

        // Create the GDT with enough space for the rest of the descriptors
        let mut gdt = vec![0x0u64; insert_idx];

        // Insert the long mode selectors
        gdt[cs_idx] = 0x00209A0000000000; // 64-bit, present, code
        gdt[ds_idx] = 0x0000920000000000; // 64-bit, present, data

        // Create a new TSS
        let mut tss: Box<Tss> = Box::new(Tss::default());

        // Create a 32 KiB critical stack for #DF, #MC and NMI
        let crit_stack: ManuallyDrop<Vec<u8>> = ManuallyDrop::new(
            Vec::with_capacity(32 * 1024));
        tss.ist[0] = crit_stack.as_ptr() as u64 + crit_stack.capacity() as u64;

        // Create the task pointer in the GDT
        let tss_base = &*tss as *const Tss as u64;
        let tss_limit = core::mem::size_of::<Tss>() as u64 -1;
        let tss_high = tss_base >> 32;
        let tss_low = 0x890000000000
            | (((tss_base >> 24) & 0xFF) << 56)
            | ((tss_base & 0xFFFFFF) << 16)
            | tss_limit;

        // Push the TSS into the GDT
        let tss_entry = (gdt.len() * 8) as u16;
        gdt.push(tss_low);
        gdt.push(tss_high);
        gdt.shrink_to_fit();
        println_shatter!("cs: {cs_idx:?} | ds: {ds_idx:?}");
        println_shatter!("{gdt:X?}");

        (Self { raw: gdt }, tss, tss_entry)
    }
}

/// Get the indices into GDT where the `(code, data)` selectors should be such
/// that the current value in the selectors will point correctly into GDT
/// without change
pub fn get_selector_indices() -> (usize, usize) {
    let mut cs: u16;
    let mut ds: u16;
    unsafe {
        core::arch::asm!(
            "mov {:x}, cs",
            "mov {:x}, ds",
            out(reg) cs,
            out(reg) ds,
        );
    }
    ((cs >> 3) as usize, (ds >> 3) as usize)
}
