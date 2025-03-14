#![no_std]
#![no_main]

use core::sync::atomic::{Ordering, AtomicBool};
use bootloader::{efi, mm, SHARED, println};
use serial::SerialDriver;
use page_table::{
    VirtAddr, PhysAddr, PageTable, MapRequest, PageType, Permissions};
use shared_data::{KERNEL_STACK_BASE, KERNEL_STACK_SIZE_PADDED};

/// Marks the bootloader as one-time initialized
static BOOTLOADER_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Do some setup on the very first initial boot of the bootloader
fn init_setup(image_handle: efi::BootloaderImagePtr,
              system_table: efi::SystemTablePtr) {
    // If the bootloader has been initialized already, exit
    if BOOTLOADER_INITIALIZED.load(Ordering::SeqCst) { return; }

    // Initialize the serial driver
    {
        let mut serial = SHARED.serial.lock();
        let driver = unsafe { SerialDriver::init() };
        *serial = Some(driver);
    }

    println!("Initializing the bootloader!");

    // Get the memory map from UEFI
    let map = unsafe {
        efi::memory_map_exit(system_table, image_handle)
            .expect("Couldn't acquire memory map from UEFI.")
    };

    // Initialize the memory manager
    mm::init(map);

    // Save the bootloader entry point
    SHARED.bootloader_entry()
        .store(efi_main as *const u8 as u64, Ordering::SeqCst);

    // Save the bootloader page table
    {
        let mut boot_pt = SHARED.bootloader_pt().lock();
        *boot_pt = unsafe { Some(PageTable::from_cr3()) };
    }

    // Validate kernel constants
    shared_data::validate_constants();

    // Mark the bootloader as initialized
    BOOTLOADER_INITIALIZED.store(true, Ordering::SeqCst);
}

/// Loads the kernel image into memory and prepares its page table
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
    SHARED.stack_ref().store(KERNEL_STACK_BASE, Ordering::SeqCst);
    let stack_base = VirtAddr(KERNEL_STACK_BASE - KERNEL_STACK_SIZE_PADDED);

    // Map the stack into kernel's memory
    let request = MapRequest::new(stack_base, PageType::Page4K,
        KERNEL_STACK_SIZE_PADDED,
        Permissions::new(true, false, false)).unwrap();
    table.map(&mut pmem, request).unwrap();
}

/// Sets up a trampoline for jumping into the kernel from the bootloader and
/// jumps to the kernel!
unsafe fn jump_to_kernel() -> ! {
    // Map in the trampoline into the bootloader and the kernel page table
    let trampoline = unsafe { bootloader::trampoline::prepare() };

    // Get the kernel entry point
    let kernel_entry = {
        let image = SHARED.kernel_image().lock();
        image.as_ref().unwrap().entry.clone()
    };

    // Get the kernel table address
    let kernel_table = {
        let table = SHARED.kernel_pt().lock();
        table.clone().unwrap()
    }.addr();

    // Get the kernel stack
    let kernel_stack = VirtAddr(SHARED.stack_ref().load(Ordering::SeqCst));
    assert!(kernel_stack.0 == KERNEL_STACK_BASE,
        "Kernel stack base not {:?} for BSP", KERNEL_STACK_BASE);

    // Get the physical address to the shared data structure so the kernel can
    // map it in wherever it wants
    let shared = PhysAddr(&SHARED as *const shared_data::Shared as u64);

    unsafe { trampoline(kernel_entry, kernel_stack, kernel_table, shared, 0); }
}

#[unsafe(no_mangle)]
extern "C" fn efi_main(image_handle: efi::BootloaderImagePtr,
                       system_table: efi::SystemTablePtr) {
    // One time bootloader initialization
    init_setup(image_handle, system_table);

    // Load the kernel image into memory
    load_kernel();

    // Set up the trampoline to the kernel and jump to it
    unsafe { jump_to_kernel() };
}
