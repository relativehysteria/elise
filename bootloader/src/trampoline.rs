use page_table::{VirtAddr, PhysAddr, PageTable, PageType, MapRequest, Permissions};
use crate::SHARED;

/// The trampoline function. This has to be identical to the function specified
/// in trampoline.asm
pub type Trampoline = unsafe extern "sysv64" fn(
    kernel_entry: VirtAddr,
    kernel_stack: VirtAddr,
    kernel_table: PhysAddr,
    shared_paddr: PhysAddr,
    core_id:      u32,
) -> !;

/// Map in the trampoline bytes into the bootloader's memory and the page
/// `table` at the same address and return a pointer to it.
///
/// This function must be called from the bootloader with the UEFI page tables
/// (that is, identity mapped memory).
pub unsafe fn prepare() -> Trampoline {
    // Get the trampoline physical and virtual addresses
    let trampoline = crate::TRAMPOLINE;
    let trampoline_virt = VirtAddr(shared_data::TRAMPOLINE_ADDR);

    {
        // Get exclusive access to physical memory
        let mut pmem = SHARED.free_memory().lock();
        let pmem = pmem.as_mut().expect("Memory still uninitialized.");
        let mut pmem = crate::mm::PhysicalMemory(pmem);

        // Build the mapping request for the trampoline
        let request = MapRequest::new(trampoline_virt,
            PageType::Page4K, trampoline.len() as u64,
            Permissions::new(false, true, false)).unwrap();

        // Create the closure that will be used to initialize the memory bytes
        let init = |offset| {
            trampoline.get(offset as usize).copied().unwrap_or(0)
        };

        // Get the kernel page table
        let mut kernel_pt = SHARED.kernel_pt().lock();
        let kernel_pt = kernel_pt.as_mut()
            .expect("Kernel page table not initialized");

        // // Map in the trampoline
        kernel_pt.map_init(&mut pmem, request.clone(), Some(init));

        // Get the bootloader page table and map the trampoline in
        unsafe {
            // UEFI will likely write protect the page table. Turn off for now.
            let mut cr0: u64;
            core::arch::asm!("mov {}, cr0", out(reg) cr0);
            core::arch::asm!("mov cr0, {}", in(reg) (cr0 & !(1 << 16)));

            // Map it in
            let mut bootloader_pt = PageTable::from_cr3();
            bootloader_pt.map_init(&mut pmem, request.clone(), Some(init));

            // Turn write protection back on
            core::arch::asm!("mov cr0, {}", in(reg) cr0);
        }
    }
    // Cast the pointer to the copied bytes as a function pointer and return
    unsafe { core::mem::transmute(trampoline_virt) }
}
