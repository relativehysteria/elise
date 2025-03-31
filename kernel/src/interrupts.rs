//! Routines and structures for manipulating x86 interrupts

use core::mem::ManuallyDrop;
use alloc::boxed::Box;
use alloc::vec::Vec;
use alloc::vec;
use core::arch::asm;

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

/// Descriptor pointer used to load with `lidt` and `lgdt`
#[repr(C, packed)]
struct TablePtr {
    limit: u16,
    base:  u64,
}

impl TablePtr {
    fn new(limit: u16, base: u64) -> Self {
        Self { limit, base }
    }
}

/// Interrupt dispatch routine.
/// Arguments are (interrupt number, frame, error code, register state at int)
///
/// Returns `true` if the interrupt was handled, and execution should continue
type InterruptDispatch =
    unsafe fn(u8, &mut InterruptFrame, u64, &mut AllRegs) -> bool;

/// Structure to hold different dispatch routines for interrupts
pub struct Interrupts {
    dispatch: [Option<InterruptDispatch>; 256],
    pub tss: Box<Tss>,
    pub idt: Vec<IdtEntry>,
    pub gdt: Vec<u64>,
}

/// Shape of a raw 64-bit interrupt frame
#[derive(Clone, Copy)]
#[repr(C, packed)]
pub struct InterruptFrame {
    pub rip:    usize,
    pub cs:     usize,
    pub rflags: usize,
    pub rsp:    usize,
    pub ss:     usize,
}

/// A raw IDT entry, which is valid when placed in an IDT in this
/// representation
#[derive(Clone, Copy)]
#[repr(C, packed)]
pub struct IdtEntry {
    offset_low:  u16,
    selector:    u16,
    ist:         u8,
    type_attr:   u8,
    offset_mid:  u16,
    offset_high: u32,
    zero:        u32,
}

impl IdtEntry {
    /// Construct a new in-memory representation of an IDT entry. This will
    /// take the `cs:offset` to the handler address, the `ist` for the
    /// interrupt stack table index, the `typ` of the IDT gate entry and the
    /// `dpl` of the IDT entry.
    fn new(cs: u16, offset: u64, ist: u32, typ: u32, dpl: u8) -> Self {
        assert!(ist <  8, "Invalid IdtEntry IST");
        assert!(typ < 32, "Invalid IdtEntry type");
        assert!(dpl <  4, "Invalid IdtEntry dpl");

        Self {
            offset_low:  (offset & 0xFFFF) as u16,
            selector:    cs,
            ist:         ist as u8,
            type_attr:   ((dpl << 5) | (1 << 7) | typ as u8),
            offset_mid:  ((offset >> 16) & 0xFFFF) as u16,
            offset_high: ((offset >> 32) & 0xFFFFFFFF) as u32,
            zero:        0,
        }
    }
}

/// Structure containing all registers at the state of the interrupt
#[derive(Clone, Copy)]
#[repr(C, packed)]
pub struct AllRegs {
    pub xmm15: u128,
    pub xmm14: u128,
    pub xmm13: u128,
    pub xmm12: u128,
    pub xmm11: u128,
    pub xmm10: u128,
    pub xmm9:  u128,
    pub xmm8:  u128,
    pub xmm7:  u128,
    pub xmm6:  u128,
    pub xmm5:  u128,
    pub xmm4:  u128,
    pub xmm3:  u128,
    pub xmm2:  u128,
    pub xmm1:  u128,
    pub xmm0:  u128,

    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9:  u64,
    pub r8:  u64,
    pub rbp: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rbx: u64,
    pub rax: u64,
}

/// Switch to a kernel-based GDT, load a TSS with a critical stack for #DF, #MC
/// and NMI interrupts and setup an IDT.
pub fn init() {
    // Get access to the interrupts. Don't reinitialize them
    let mut interrupts = unsafe { core!().interrupts().lock() };
    assert!(interrupts.is_none(), "Interrupts already initialized!");

    // Create a new TSS
    let mut tss: Box<Tss> = Box::new(Tss::default());

    // Create a 32 KiB critical stack for #DF, #MC and NMI
    let crit_stack: ManuallyDrop<Vec<u8>> = ManuallyDrop::new(
        Vec::with_capacity(32 * 1024));
    tss.ist[0] = crit_stack.as_ptr() as u64 + crit_stack.capacity() as u64;

    // Create a kernel GDT; TODO: save the bootloader GDT
    // If you ever move the kernel code segment from 0x8, don't forget to update
    // the IDT entries below
    let mut gdt: Vec<u64> = vec![
        0x0000000000000000, // 0x00 | null
        0x00209a0000000000, // 0x08 | 64-bit, present, code, base 0
        0x0000920000000000, // 0x10 | 64-bit, present, data, base 0
    ];

    // Create the task pointer in the GDT
    let tss_base = &*tss as *const Tss as u64;
    let tss_low = 0x890000000000 | (((tss_base >> 24) & 0xff) << 56) |
        ((tss_base & 0xffffff) << 16) |
        (core::mem::size_of::<Tss>() as u64 - 1);
    let tss_high = tss_base >> 32;

    // Push the TSS to into the GDT
    let tss_entry = (gdt.len() * 8) as u16;
    gdt.push(tss_low);
    gdt.push(tss_high);

    // Create a pointer to the GDT for lgdt to load
    let gdt_ptr = TablePtr::new(
        core::mem::size_of_val(&gdt[..]) as u16 - 1,
        gdt.as_ptr() as u64);

    // Update the GDT
    unsafe {
        asm!(
            // Load the GDT
            "lgdt [{0}]",

            // Load the TSS
            "mov cx, {1:x}",
            "ltr cx",

            in(reg) &gdt_ptr as *const TablePtr,
            in(reg) tss_entry,
            out("rcx") _,
            options(nostack, preserves_flags)
        );
    }

    // Create a new IDT
    let mut idt = Vec::with_capacity(256);
    for id in 0..idt.capacity() {
        let ist = match id {
            // NMI, #DF, #MC use the IST
            2 | 8 | 18 => { 1 },

            // The rest uses the existing stack
            _ => { 0 },
        };

        /// Interrupt gate type for 64-bit mode
        const X64_INTERRUPT_GATE: u32 = 0xE;
        let handler_addr = default_interrupt_handler as u64;

        // Construct the IDT entry pointing to the default handler
        idt.push(IdtEntry::new(
            0x08,               // Kernel code segment
            handler_addr,       // Address of the handler
            ist,                // IST index
            X64_INTERRUPT_GATE, // Type (interrupt gate)
            0                   // DPL
        ));
    }

    // Make sure the entire IDT is present fully
    assert!(core::mem::size_of_val(&idt[..]) == 4096);

    // Load the idt
    let idt_ptr = TablePtr::new(0xfff, idt.as_ptr() as u64);
    unsafe {
        asm!(
            "lidt [{0}]", in(reg) &idt_ptr as *const TablePtr,
            options(nostack, preserves_flags)
        );
    }

    // Create the interrupts structure
    *interrupts = Some(Interrupts { dispatch: [None; 256], gdt, idt, tss });
}

unsafe extern "C" fn default_interrupt_handler(
    _interrupt_number: u8,
    _frame: &mut InterruptFrame,
    _error_code: u64,
    _regs: &mut AllRegs,
) -> bool {
    println_shatter!("Unhandled interrupt: {}", _interrupt_number);
    true
}
