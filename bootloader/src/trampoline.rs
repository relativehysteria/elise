use core::sync::atomic::{AtomicU64, Ordering};
use page_table::{VirtAddr, PhysAddr, PageTable, PageType, MapRequest, Permissions};
use shared_data::TRAMPOLINE_ADDR;
use crate::SHARED;

/// The trampoline function. This has to be identical to the function specified
/// in trampoline.asm
pub type Trampoline = unsafe extern "sysv64" fn(
    entry:   VirtAddr,
    stack:   VirtAddr,
    table:   PhysAddr,
    paddr:   PhysAddr,
    core_id: u32,
) -> !;

/// The raw page table entry for the trampoline. This entry can be used to map
/// the trampoline to page tables without duplicating the bytes in physical
/// memory.
///
/// `0` means uninitialized.
static RAW_PT_ENTRY: AtomicU64 = AtomicU64::new(0);

/// Maps the trampoline into the current page table and sets [`RAW_PT_ENTRY`] to
/// the raw page table entry of the mapping. Does nothing if the trampoline has
/// been mapped (and therefore is present in physical memory) already.
pub fn map_once() {
    // Avoid writing the bytes into physical memory again
    if RAW_PT_ENTRY.load(Ordering::SeqCst) != 0 { return; }

    // Get the trampoline bytes and its target virtual address
    let trampoline      = crate::TRAMPOLINE;
    let trampoline_virt = VirtAddr(TRAMPOLINE_ADDR);

    // Build the mapping request for the trampoline
    let request = MapRequest::new(
        trampoline_virt,
        PageType::Page4K,
        trampoline.len() as u64,
        Permissions::new(false, true, false)
    ).expect("Failed to create map request");

    // Create the closure that will be used to initialize the memory bytes
    let init = |offset| trampoline.get(offset as usize).copied().unwrap_or(0);

    // Acquire exclusive access to physical memory
    let mut pmem = SHARED.get().free_memory().lock();
    let pmem = pmem.as_mut().expect("Memory still uninitialized.");
    let mut pmem = crate::mm::PhysicalMemory(pmem);

    // Get the current page table
    let mut table = unsafe { PageTable::from_cr3() };

    // Map the trampoline into the current page table
    unsafe {
        // UEFI will likely write protect the bootloader page table. Disable WP.
        let mut cr0: u64;
        core::arch::asm!("mov {}, cr0", out(reg) cr0);
        core::arch::asm!("mov cr0, {}", in(reg) (cr0 & !(1 << 16)));

        // Map the trampoline in
        table.map_init(&mut pmem, request, Some(init))
            .expect("Couldn't map in the trampoline");

        // Re-enable write protection.
        core::arch::asm!("mov cr0, {}", in(reg) cr0);
    }

    // Initialize the static page table entry for generic use
    let raw = table.components(&mut pmem, trampoline_virt)
        .expect("Couldn't get the trampoline page table mapping components")
        .page.expect("Couldn't get the raw page entry for the trampoline").2;
    RAW_PT_ENTRY.store(raw, Ordering::SeqCst);
}

/// Maps the trampoline in the kernel's page table and returns a pointer to it.
///
/// Returns `None` if the trampoline wasn't mapped into physical memory using
/// `map_once()` before this call.
pub unsafe fn prepare() -> Option<Trampoline> {
    // Get the trampoline raw page table entry
    let trampoline_raw = RAW_PT_ENTRY.load(Ordering::SeqCst);
    if trampoline_raw == 0 { return None; }

    // Acquire exclusive access to physical memory
    let mut pmem = SHARED.get().free_memory().lock();
    let pmem = pmem.as_mut().expect("Memory still uninitialized.");
    let mut pmem = crate::mm::PhysicalMemory(pmem);

    // Map in the raw trampoline page entry in the kernel page table
    let mut table = SHARED.get().kernel_pt().lock();
    let table = table.as_mut().expect("Kernel table uninitialized");

    unsafe {
        table.map_raw(
            &mut pmem,
            VirtAddr(TRAMPOLINE_ADDR),
            PageType::Page4K, trampoline_raw
        ).expect("Failed ot map in the raw trampoline page table entry");
    }

    // Return the function pointer
    Some(unsafe { core::mem::transmute(TRAMPOLINE_ADDR) })
}
