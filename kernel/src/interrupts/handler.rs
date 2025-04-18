//! Defined interrupt handlers

#![allow(unused_variables)]

use crate::interrupts::InterruptArgs;

/// NMI handler
pub unsafe fn nmi(args: InterruptArgs) -> bool {
    false
}

/// Page Fault handler
pub unsafe fn page_fault(args: InterruptArgs) -> bool {
    false
}
