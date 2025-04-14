use page_table::{VirtAddr, PageTable};
use crate::Shared;

/// The trampoline function. This has to be identical to the function specified
/// in trampoline.asm
pub type Trampoline = unsafe extern "sysv64" fn(
    entry: VirtAddr,
    stack: VirtAddr,
    table: PageTable,
    shared: *const Shared,
) -> !;

/// Returns a pointer to the trampoline.
///
/// The trampoline must be mapped in the current page table at `TRAMPOLINE_ADDR`
pub unsafe fn get_trampoline() -> Trampoline {
    unsafe { core::mem::transmute(crate::TRAMPOLINE_ADDR) }
}
