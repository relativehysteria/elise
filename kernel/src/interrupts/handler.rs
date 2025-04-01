//! Defined interrupt handlers

#![allow(unused_variables)]

use crate::interrupts::{InterruptFrame, AllRegs};

/// NMI handler
pub unsafe fn nmi(
    number: u8,
    frame: &mut InterruptFrame,
    error: u64,
    regs: &mut AllRegs,
) -> bool {
    false
}

/// Page Fault handler
pub unsafe fn page_fault(
    number: u8,
    frame: &mut InterruptFrame,
    error: u64,
    regs: &mut AllRegs,
) -> bool {
    false
}
