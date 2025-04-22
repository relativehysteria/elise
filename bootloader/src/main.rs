#![no_std]
#![no_main]

use core::sync::atomic::{AtomicBool, Ordering};
use bootloader::{efi, mm, trampoline, SHARED, println};
use serial::SerialDriver;
use page_table::{
    VirtAddr, PageTable, MapRequest, PageType, Permissions,
    PAGE_PRESENT, PAGE_WRITE, PAGE_SIZE};
use shared_data::{
    KERNEL_STACK_SIZE_PADDED, KERNEL_PHYS_WINDOW_BASE, KERNEL_PHYS_WINDOW_SIZE,
    BootloaderState};
use oncelock::OnceLock;
use rangeset::RangeSet;

/// Whenever the kernel soft reboots and jumps back to our bootloader, this is
/// what the memory map state will become
static MEMORY_SNAPSHOT: OnceLock<RangeSet> = OnceLock::new();

/// Whether the bootloader has been initialized
static INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Do some setup on the very first initial boot of the bootloader.
fn init_setup(image_handle: efi::BootloaderImagePtr,
              system_table: efi::SystemTablePtr) {
    // Initialize the serial driver
    let mut serial = unsafe { SerialDriver::init() };
    serial.write("─────────────────────────────┐\n".as_bytes());
    serial.write("Initializing the bootloader! │\n\n".as_bytes());

    // Retrieve the UEFI memory map
    let map = unsafe {
        efi::memory_map_exit(system_table, image_handle)
            .expect("Couldn't acquire memory map from UEFI.")
    };

    // Save the serial driver
    *SHARED.serial.lock() = Some(serial);

    // Initialize the memory manager
    mm::init(map);

    // Map in the trampoline into the bootloader memory space
    trampoline::map_once();

    // Get the bootloader memory snapshot, page table, entry point and stack
    let memory = SHARED.free_memory().lock().as_ref().unwrap().clone();
    let page_table = unsafe { PageTable::from_cr3() };
    let entry = VirtAddr(efi_main as *const u8 as u64);
    let stack = VirtAddr(unsafe {
        let stack: u64;
        core::arch::asm!("mov {}, rsp", out(reg) stack);
        stack
    });

    // Retrieve the SDT table and save it
    let sdt = unsafe { efi::acpi::get_sdt_table(system_table) }
        .expect("Couldn't retrieve the SDT table.");
    SHARED.acpi().set(sdt);

    // Take a snapshot of the bootloader in its current state.
    // This snapshot is what we'll return to when the kernel soft-reboots
    SHARED.bootloader().set(BootloaderState { page_table, entry, stack });

    // Save the memory snapshot as well
    MEMORY_SNAPSHOT.set(memory);

    // Mark the bootloader as initialized
    INITIALIZED.store(true, Ordering::SeqCst);
}

/// Restores the physical memory to the snapshot taken during initialization.
///
/// # SAFETY
/// * It must be called _after_ bootloader initialization.
/// * Any memory allocated _after_ initialization will be freed, meaning any
///   virtual mappings made after initialization will become invalid and as such
///   this function is inherently unsafe.
unsafe fn restore_physical_memory() {
    // Restore physical memory
    *SHARED.free_memory().lock() = Some(MEMORY_SNAPSHOT.get().clone());
}

/// Loads the kernel image into memory and prepares its page tables
fn load_kernel() {
    println!("Loading the kernel image!");

    // If there wasn't a kernel image loaded yet, this is the inital boot. Set
    // it to the embedded kernel image
    let mut kernel = SHARED.kernel_image().lock();
    if kernel.is_none() {
        println!("No kernel image found. Using the embedded one from now on.");

        // Parse the embedded kernel
        *kernel = Some(elf_parser::Elf::parse(bootloader::INITIAL_KERNEL_IMAGE)
            .expect("Couldn't parse embedded kernel image."));
    }

    // Get exclusive access to physical memory so we can write the kernel
    // to where it wants
    let mut pmem = SHARED.free_memory().lock();
    let pmem = pmem.as_mut().expect("Memory not still uninitialized.");
    let mut pmem = mm::PhysicalMemory(pmem);

    // Create the page table for the kernel
    let mut kernel_table = SHARED.kernel_pt().lock();
    *kernel_table = Some(PageTable::new(&mut pmem)
        .expect("Failed to create the kernel page table."));
    let table = kernel_table.as_mut().unwrap();

    println!("────────────────────────────────────────────────────────────");
    println!("Mapping in the kernel segments.");

    // Map in the kernel to where it expects
    for segment in kernel.as_ref().unwrap().segments() {
        let segment = segment
            .expect("Segment failed while creating the kernel page table.");

        println!("\n{:?}", segment.permissions);
        println!(" ├ Vaddr:  {:X?}", segment.vaddr);
        println!(" ├ Vsize:  0x{:X?}", segment.vsize);
        println!(" └ Offset: 0x{:X?}", segment.offset);

        // Get the memory permissions for this segment
        let perms = Permissions::new(
            segment.permissions.write,
            segment.permissions.execute,
            false);

        // Create the mapping request
        let request = MapRequest::new(segment.vaddr, PageType::Page4K,
            segment.offset + segment.vsize, perms)
        .expect("Error while requesting a map in");

        // Map in the request, initializing it to the kernel bytes at the
        // correct offset
        table.map_init(&mut pmem, request, Some(|mem_offset| {
            if mem_offset >= segment.offset {
                segment.bytes.get((mem_offset - segment.offset) as usize)
                    .copied().unwrap_or(0)
            } else { 0 }
        }));
    }
    println!();

    // This is a fresh kernel launch, set the stack back to its base
    SHARED.reset_stack();

    // Map the trampoline in as well. First, make sure it's been mapped into
    // physical memory already
    let trampoline_raw = trampoline::RAW_PT_ENTRY.load(Ordering::SeqCst);
    if trampoline_raw == 0 {
        panic!("The trampoline hasn't been mapped in yet!");
    }

    // Then map the raw page table entry into the kernel page table
    unsafe {
        table.map_raw(
            &mut pmem,
            VirtAddr(shared_data::TRAMPOLINE_ADDR),
            PageType::Page4K,
            trampoline_raw)
        .expect("Failed to map in the raw trampoline page table entry");
    }

    // Map in the physical memory window
    //
    // First, get CPU features to know which pages we can use
    let features = cpu::Features::get();

    // Determine the corresponding page type
    let page_type = if features.gbyte_pages {
        PageType::Page1G
    } else if features.pse {
        PageType::Page2M
    } else {
        PageType::Page4K
    };

    // Get the raw page table entry mask for this entry
    let page_mask = 0 | PAGE_WRITE | PAGE_PRESENT | match page_type {
        PageType::Page4K => 0,
        _ => PAGE_SIZE,
    };

    // Map the window in!
    for paddr in (0..KERNEL_PHYS_WINDOW_SIZE).step_by(page_type as usize) {
        let virt = VirtAddr(KERNEL_PHYS_WINDOW_BASE + paddr);
        unsafe {
            table.map_raw(&mut pmem, virt, page_type, paddr | page_mask)
                .unwrap();
        }
    }
}

/// Sets up a trampoline for jumping into the kernel from the bootloader and
/// jumps to the kernel!
unsafe fn jump_to_kernel(stack: VirtAddr) {
    // Get the pointer to the trampoline
    let trampoline = unsafe { shared_data::get_trampoline() };

    // Get the kernel stuff
    let entry = SHARED.kernel_image().lock().as_ref().unwrap().entry;
    let table = SHARED.kernel_pt().lock().as_ref().unwrap().clone();
    let shared = page_table::PhysAddr(&SHARED as *const _ as u64);

    println!("ENTERING KERNEL ────────────────────────────────────────────");

    unsafe { trampoline(entry, stack, table, shared); }
}

/// Maps in a new stack into the kernel's memory and return the base where it's
/// been mapped
unsafe fn map_kernel_stack() -> VirtAddr {
    // Get exclusive access to the kernel page table
    let mut table = SHARED.kernel_pt().lock();
    let table = table.as_mut().unwrap();

    // Get exclusive access to physical memory
    let mut pmem = SHARED.free_memory().lock();
    let pmem = pmem.as_mut().expect("Memory not still uninitialized.");
    let mut pmem = mm::PhysicalMemory(pmem);

    // Get the base for a new stack
    let stack_base = VirtAddr(SHARED.get_next_stack()
        .expect("Out of stacks to map"));

    // Map the stack into kernel's memory
    let request = MapRequest::new(
        stack_base,
        PageType::Page4K,
        KERNEL_STACK_SIZE_PADDED,
        Permissions::new(true, false, false)
    ).unwrap();
    table.map(&mut pmem, request).unwrap();

    // Return the base which should be put into RSP. This addition shouldn't
    // fail because it's checked in `get_next_stack()`.
    VirtAddr(stack_base.0.checked_add(KERNEL_STACK_SIZE_PADDED).unwrap())
}

/// This is the entry point for both the bootloader itself (the one that UEFI
/// passes execution to) and for all of the cores on the system when the kernel
/// brings them up (at which point the arguments of this function won't be used)
#[unsafe(no_mangle)]
unsafe extern "C" fn efi_main(image_handle: efi::BootloaderImagePtr,
                              system_table: efi::SystemTablePtr) {
    // One time bootloader initialization.
    if !INITIALIZED.load(Ordering::SeqCst) {
        init_setup(image_handle, system_table);
    }

    // From now on, due to `restore_physical_memory()`, all bootloader virtual
    // mappings are locked in and no more must be done. We are still free to
    // use physical memory when doing kernel mappings and such.

    // If we're rebooting, load the kernel into memory. The `rebooting` variable
    // is initialized as `true`, so if the bootloader runs for the first time,
    // this path will be hit
    if SHARED.is_rebooting() {
        unsafe { restore_physical_memory(); }
        load_kernel();
        SHARED.rebooting.store(false, Ordering::SeqCst);
    }

    // Map in a new stack for this core
    let stack = unsafe { map_kernel_stack() };

    // Set up the trampoline to the kernel and jump to it
    unsafe { jump_to_kernel(stack) };
}
