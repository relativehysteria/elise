#![no_std]
#![no_main]

use core::sync::atomic::Ordering;
use bootloader::{efi, mm, trampoline, SHARED, println, print};
use serial::SerialDriver;
use page_table::{
    VirtAddr, PageTable, MapRequest, PageType, Permissions,
    PAGE_PRESENT, PAGE_WRITE, PAGE_SIZE, PAGE_NXE};
use shared_data::{
    KERNEL_STACK_BASE, KERNEL_SHARED_BASE, KERNEL_STACK_SIZE_PADDED,
    KERNEL_PHYS_WINDOW_BASE, KERNEL_PHYS_WINDOW_SIZE,
    BootloaderState, Shared};
use oncelock::OnceLock;
use rangeset::RangeSet;

/// Whenever the kernel soft reboots and jumps back to our bootloader, this is
/// what the memory map state will become
static MEMORY_SNAPSHOT: OnceLock<RangeSet> = OnceLock::new();

/// Do some setup on the very first initial boot of the bootloader.
/// Returns `true` if the kernel was already set up.
fn init_setup(image_handle: efi::BootloaderImagePtr,
              system_table: efi::SystemTablePtr) -> bool {
    // If the bootloader has been initialized already, exit
    if SHARED.initialized() {
        return true;
    }

    // Initialize the serial driver
    let mut serial = unsafe { SerialDriver::init() };
    serial.write("─────────────────────────────┐\n".as_bytes());
    serial.write("Initializing the bootloader! │\n\n".as_bytes());

    // Retrieve the UEFI memory map
    let mut map = unsafe {
        efi::memory_map_exit(system_table, image_handle)
            .expect("Couldn't acquire memory map from UEFI.")
    };

    // Allocate the SHARED data structure. Because UEFI sets up the page tables
    // to be unit mapped, the address here is both physical and virtual
    use page_table::PhysMem;
    let mut pmem = mm::PhysicalMemory(&mut map);
    let shared_addr = pmem.alloc_phys_zeroed(
        core::alloc::Layout::from_size_align(
            core::mem::size_of::<Shared>(),
            4096).unwrap()
        ).unwrap();

    // Initialize the SHARED struct and save it as global!
    let shared_ptr: *mut Shared = shared_addr.0 as *mut Shared;
    unsafe { shared_ptr.write(Shared::new()); }
    SHARED.set(unsafe { &*shared_ptr });

    // Save the serial driver
    *SHARED.get().serial.lock() = Some(serial);

    // Initialize the memory manager
    mm::init(map);

    // Map in the trampoline into the bootloader memory space
    trampoline::map_once();

    // Get the bootloader memory snapshot, page table, entry point and stack
    let memory = SHARED.get().free_memory().lock().as_ref().unwrap().clone();
    let page_table = unsafe { PageTable::from_cr3() };
    let entry = VirtAddr(efi_main as *const u8 as u64);
    let stack = VirtAddr(unsafe {
        let stack: u64;
        core::arch::asm!("mov {}, rsp", out(reg) stack);
        stack
    });

    // Take a snapshot of the bootloader in its current state and mark the
    // bootloader as initialized. This snapshot is what we'll return to when the
    // kernel soft-reboots
    SHARED.get().bootloader().set(BootloaderState { page_table, entry, stack });

    // Save the memory snapshot as well
    MEMORY_SNAPSHOT.set(memory);

    false
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
    *SHARED.get().free_memory().lock() = Some(MEMORY_SNAPSHOT.get().clone());
}

/// Loads the kernel image into memory and prepares its page tables
fn load_kernel() {
    println!("Loading the kernel image!");

    // If there wasn't a kernel image loaded yet, this is the inital boot. Set
    // it to the embedded kernel image
    let mut kernel = SHARED.get().kernel_image().lock();
    if kernel.is_none() {
        println!("No kernel image found. Using the embedded one from now on.");

        // Parse the embedded kernel
        *kernel = Some(elf_parser::Elf::parse(bootloader::INITIAL_KERNEL_IMAGE)
            .expect("Couldn't parse embedded kernel image."));
    }

    // Get exclusive access to physical memory so we can write the kernel
    // to where it wants
    let mut pmem = SHARED.get().free_memory().lock();
    let pmem = pmem.as_mut().expect("Memory not still uninitialized.");
    let mut pmem = mm::PhysicalMemory(pmem);

    // Create the page table for the kernel
    let mut kernel_table = SHARED.get().kernel_pt().lock();
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
    SHARED.get().stack().store(KERNEL_STACK_BASE, Ordering::SeqCst);
    let stack_base = VirtAddr(KERNEL_STACK_BASE - KERNEL_STACK_SIZE_PADDED);

    // Map the stack into kernel's memory
    let request = MapRequest::new(
        stack_base,
        PageType::Page4K,
        KERNEL_STACK_SIZE_PADDED,
        Permissions::new(true, false, false)
    ).unwrap();
    table.map(&mut pmem, request).unwrap();

    // Map in the SHARED data structure
    print!("Mapping the SHARED structure into kernel memory.\nPages: ");
    let phys_addr = *SHARED.get() as *const Shared as u64;
    for offset in (0..core::mem::size_of::<Shared>()).step_by(0x1000) {
        print!("0x{:x?} ", phys_addr + offset as u64);
        unsafe {
            table.map_raw(
                &mut pmem,
                VirtAddr(KERNEL_SHARED_BASE + offset as u64),
                PageType::Page4K,
                (phys_addr + offset as u64)
                    | PAGE_PRESENT | PAGE_WRITE | PAGE_NXE
            ).expect("Couldn't map the SHARED structure into kernel.");
        }
    }
    println!();

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
unsafe fn jump_to_kernel() {
    // Map in the trampoline in the kernel page table and get a pointer to it
    let trampoline = unsafe { bootloader::trampoline::prepare().unwrap() };

    // Get the kernel entry point, page table and stack addresses
    let entry = SHARED.get().kernel_image().lock().as_ref().unwrap().entry;
    let table = SHARED.get().kernel_pt().lock().as_ref().unwrap().clone();
    let stack = VirtAddr(SHARED.get().stack().load(Ordering::SeqCst));

    // Make sure the stack is at its base
    assert!(stack.0 == KERNEL_STACK_BASE,
        "Kernel stack base not {:?} for BSP", KERNEL_STACK_BASE);

    println!("────────────────────────────────────────────────────────────");
    println!("Jumping into kernel!");
    println!(" ├ entry:  {:X?}", entry);
    println!(" ├ stack:  {:X?}", stack);
    println!(" └ table:  {:X?}", table);

    unsafe { trampoline(entry, stack, table, 0); }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn efi_main(image_handle: efi::BootloaderImagePtr,
                       system_table: efi::SystemTablePtr) {
    // One time bootloader initialization. If the bootloader was set up already,
    // restore to physical memory to the snapshot we took on initialization
    if init_setup(image_handle, system_table) {
        unsafe { restore_physical_memory(); }
    }

    // From now on, due to `restore_physical_memory()`, all bootloader virtual
    // mappings are locked in and no more must be done. We are still free to
    // use physical memory when doing kernel mappings and such.

    // Load the kernel image into memory
    load_kernel();

    // Set up the trampoline to the kernel and jump to it
    unsafe { jump_to_kernel() };
}
