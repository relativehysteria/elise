//! Routines and structures for manipulating x86 interrupts

use alloc::boxed::Box;
use alloc::vec::Vec;
use alloc::vec;
use core::arch::asm;
use core::mem::ManuallyDrop;
use crate::interrupts::{handler, INT_HANDLERS, AllRegs};

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

#[derive(Clone, Copy, Debug)]
/// The interrupt information passed to all interrupt handlers
pub struct InterruptArgs<'a> {
    /// The interrupt vector identifier
    pub number: u8,

    /// The interrupt frame passed by the CPU to the handler
    pub frame: &'a InterruptFrame,

    /// The error number if the interrupt is an exception
    pub error: u64,

    /// The register snapshot at the point of the interrupt
    pub regs: &'a AllRegs,
}

impl<'a> InterruptArgs<'a> {
    #[inline]
    /// Create a new
    pub fn new(n: u8, f: &'a InterruptFrame, e: u64, r: &'a AllRegs) -> Self {
        Self { number: n, frame: f, error: e, regs: r }
    }
}

/// Interrupt dispatch routine.
/// Arguments are (interrupt number, frame, error code, register state at int)
///
/// Returns `true` if the interrupt was handled, and execution should continue
type InterruptDispatch = unsafe fn(InterruptArgs) -> bool;

/// Structure to hold different dispatch routines for interrupts
pub struct Interrupts {
    dispatch: [Option<InterruptDispatch>; 256],
    pub tss: Box<Tss>,
    pub idt: Vec<IdtEntry>,
    pub gdt: Vec<u64>,
}

impl Interrupts {
    /// Register an interrupt handler
    fn register(&mut self, interrupt_number: u8, handler: InterruptDispatch) {
        self.dispatch[interrupt_number as usize] = Some(handler);
    }
}

/// Shape of a raw 64-bit interrupt frame
#[derive(Clone, Copy, Debug)]
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

    // Create a kernel GDT. Since the bootloader is in long mode, we should be
    // able to use the kernel GDT with the bootloader as well.
    // If you ever change anything, don't forget to update the IDT entries below
    let mut gdt: Vec<u64> = vec![
        0x0000000000000000, // 0x00 | null
        0x00009A008000FFFF, // 0x08 | 16-bit, present, code, base 0x8000
        0x000092000000FFFF, // 0x10 | 16-bit, present, data, base 0
        0x00cF9A000000FFFF, // 0x18 | 32-bit, present, code, base 0
        0x00CF92000000FFFF, // 0x20 | 32-bit, present, data, base 0
        0x00209A0000000000, // 0x28 | 64-bit, present, code, base 0
        0x0000920000000000, // 0x30 | 64-bit, present, data, base 0
    ];

    // Create the task pointer in the GDT
    let tss_base = &*tss as *const Tss as u64;
    let tss_low = 0x890000000000 | (((tss_base >> 24) & 0xFF) << 56) |
        ((tss_base & 0xFFFFFF) << 16) |
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

        let handler_addr = INT_HANDLERS[id] as u64;

        // Construct the IDT entry pointing to the default handler
        idt.push(IdtEntry::new(
            0x28,               // Kernel code segment in the GDT
            handler_addr,       // Address of the handler for all interrupts
            ist,                // IST index
            X64_INTERRUPT_GATE, // Type (interrupt gate)
            0                   // DPL
        ));
    }

    // Make sure the entire IDT is present fully
    assert!(core::mem::size_of_val(&idt[..]) == 4096);

    // Load the idt
    let idt_ptr = TablePtr::new(0xFFF, idt.as_ptr() as u64);
    unsafe {
        asm!(
            "lidt [{0}]", in(reg) &idt_ptr as *const TablePtr,
            options(nostack, preserves_flags)
        );
    }

    // Create the interrupts structure and register our handlers
    let mut ints = Interrupts { dispatch: [None; 256], gdt, idt, tss };
    ints.register(0x2, handler::nmi);
    ints.register(0xE, handler::page_fault);

    *interrupts = Some(ints);
}

#[unsafe(no_mangle)]
unsafe extern "sysv64" fn interrupt_entry(
    number: u8,
    frame: &InterruptFrame,
    error: u64,
    regs: &AllRegs,
) {
    // TODO:
    // * EOI
    // * Drain before soft reboot
    // * enter exception/interrupt

    let args = InterruptArgs::new(number, frame, error, regs);

    // Dispatch the interrupt if applicable
    let handled = unsafe {
        core!().interrupts().lock().as_ref().unwrap()
            .dispatch[number as usize]
            .map_or(false, |handler| handler(args))
    };

    // If the interrupt was not handled, panic
    if !handled { unhandled(args); }
}

#[inline(always)]
fn unhandled(args: InterruptArgs) -> ! {

    /// Macro to copy unaligned fields from a packed struct.
    macro_rules! regs {
        ($regs:expr, $($field:ident),*) => { ($($regs.$field,)*) };
    }

    let (rax, rcx, rdx, rbx, rbp, rsi, rdi, r8, r9, r10, r11, r12, r13,
        r14, r15, xmm0, xmm1, xmm2, xmm3, xmm4, xmm5, xmm6, xmm7, xmm8, xmm9,
        xmm10, xmm11, xmm12, xmm13, xmm14, xmm15) = regs!(args.regs,
        rax, rcx, rdx, rbx, rbp, rsi, rdi, r8, r9, r10, r11, r12, r13, r14,
        r15, xmm0, xmm1, xmm2, xmm3, xmm4, xmm5, xmm6, xmm7, xmm8, xmm9,
        xmm10, xmm11, xmm12, xmm13, xmm14, xmm15);

    let (rsp, rfl, rip) = regs!(args.frame, rsp, rflags, rip);

    let core_id = core!().id;
    let cr2 = cpu::read_cr2();

    let number = args.number;
    let error = args.error;

    panic!(r#"
Unhandled interrupt <{number:#X}>, error code <{error:#X}> on core <{core_id}>
 ┌────────────────────────────────────────────────────────────────────────────────────
 ├ rax {rax:016X} rcx {rcx:016X} rdx {rdx:016X} rbx {rbx:016X}
 ├ rsp {rsp:016X} rbp {rbp:016X} rsi {rsi:016X} rdi {rdi:016X}
 ├ r8  {r8:016X} r9  {r9:016X} r10 {r10:016X} r11 {r11:016X}
 ├ r12 {r12:016X} r13 {r13:016X} r14 {r14:016X} r15 {r15:016X}
 │
 ├ rip {rip:016X} rfl {rfl:016X} cr2 {cr2:016X}
 │
 ├ xmm0  {xmm0:032X} xmm1  {xmm1:032X}
 ├ xmm2  {xmm2:032X} xmm3  {xmm3:032X}
 ├ xmm4  {xmm4:032X} xmm5  {xmm5:032X}
 ├ xmm6  {xmm6:032X} xmm7  {xmm7:032X}
 ├ xmm8  {xmm8:032X} xmm9  {xmm9:032X}
 ├ xmm10 {xmm10:032X} xmm11 {xmm11:032X}
 ├ xmm12 {xmm12:032X} xmm13 {xmm13:032X}
 └ xmm14 {xmm14:032X} xmm15 {xmm15:032X}
"#);
}
