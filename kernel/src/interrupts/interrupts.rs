//! Routines and structures for manipulating x86 interrupts

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use core::arch::asm;

use crate::interrupts::{
    handler, INT_HANDLERS, AllRegs, Gdt, Tss, get_selector_indices};
use crate::apic::LocalApic;

/// Indicates whether the interrupt number at index into this array requires an
/// EOI when handled
pub static EOI_REQUIRED: [AtomicBool; 256] =
    [const { AtomicBool::new(false) }; 256];

/// Inidicates whether we're draining interrupts instead of handling them.
///
/// This has to be set before a soft reboot such that we drain all pending
/// interrupts before we shut down the APIC and the kernel.
pub static DRAINING_EOIS: AtomicBool = AtomicBool::new(false);

/// Indicates whether this interrupt is supposed to get handled even when we're
/// draining EOIs
static DRAIN_PRECEDENCE: [AtomicBool; 256] =
    [const { AtomicBool::new(false) }; 256];

/// Returns the bitmask of the `EOI_REQUIRED` array.
///
/// Each bit set means that the IDT entry at that bit index requires an EOI.
///
/// There's a race condition; if the `EOI_REQUIRED` array is updated while this
/// function runs, it might return stale data, and as such is marked unsafe.
pub unsafe fn eoi_bitmask() -> [u128; 2] {
    let mut bitmask = [0; 2];

    for (i, eoi) in EOI_REQUIRED.iter().enumerate() {
        let idx = i / 128;
        let bit = i % 128;
        bitmask[idx] |= (eoi.load(Ordering::SeqCst) as u128) << bit;
    }

    bitmask
}

/// The interrupt information passed to all interrupt handlers
#[derive(Clone, Copy, Debug)]
pub struct InterruptArgs<'a> {
    /// The interrupt vector identifier
    pub id: InterruptId,

    /// The interrupt frame passed by the CPU to the handler
    pub frame: &'a InterruptFrame,

    /// The error number if the interrupt is an exception
    pub error: u64,

    /// The register snapshot at the point of the interrupt
    pub regs: &'a AllRegs,
}

impl<'a> InterruptArgs<'a> {
    #[inline]
    /// Wrap the interrput information into this struct
    pub fn new(id: InterruptId, frame: &'a InterruptFrame, error: u64,
            regs: &'a AllRegs) -> Self {
        Self { id, frame, error, regs }
    }

    /// Returns whether this interrupt is an exception
    pub fn is_exception(&self) -> bool {
        (self.id as u8) < 32
    }
}

/// Interrupt dispatch routine.
///
/// Returns `true` if the interrupt was handled, and execution should continue
type InterruptDispatch = unsafe fn(InterruptArgs) -> bool;

/// Structure to hold different dispatch routines for interrupts
pub struct Interrupts {
    dispatch: [Option<InterruptDispatch>; 256],
    pub tss: Box<Tss>,
    pub idt: Vec<IdtEntry>,
    pub gdt: Gdt,
}

impl Interrupts {
    /// Register an interrupt handler
    #[track_caller]
    pub fn register(&mut self, id: InterruptId, handler: InterruptDispatch,
            eoi: bool) {
        let idx = id as usize;

        // Do not register any handler for reserved interrupts
        assert!(id < InterruptId::Reserved || id > InterruptId::LastReserved,
            "Can't register handler for reserved interrupts.");

        // Re-registering an interrupt handler at runtime is undefined behavior
        assert!(self.dispatch[idx].is_none(),
            "Interrupt handler already installed for {:?}", id);

        // Register the handler
        self.dispatch[idx] = Some(handler);

        // Register whether EOI is required when handling this interrupt
        EOI_REQUIRED[idx].store(eoi, Ordering::SeqCst);
    }

    /// Register an interrupt handler that gets handled even when EOIs are being
    /// drained
    #[track_caller]
    pub fn register_precedent(&mut self, id: InterruptId,
            handler: InterruptDispatch, eoi: bool) {
        self.register(id, handler, eoi);

        // Register that this interrupt gets handled even during EOI draining
        DRAIN_PRECEDENCE[id as usize].store(true, Ordering::SeqCst);
    }

    /// Unregister an interrupt handler
    pub fn unregister(&mut self, id: InterruptId) {
        let idx = id as usize;
        self.dispatch[idx] = None;
        EOI_REQUIRED[idx].store(false, Ordering::SeqCst);
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
#[derive(Clone, Copy, Debug)]
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
    fn new(cs: u16, offset: usize, ist: u32, typ: u32, dpl: u8) -> Self {
        assert!(ist <  8, "Invalid IdtEntry IST");
        assert!(typ < 32, "Invalid IdtEntry type");
        assert!(dpl <  4, "Invalid IdtEntry dpl");

        let offset = offset as u64;

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

/// Switch to a kernel-based GDT, load a TSS with a critical stack for #DF, #MC
/// and NMI interrupts and setup an IDT.
pub fn init() {
    // Get access to the interrupts. Don't reinitialize them
    let mut interrupts = unsafe { core!().interrupts().lock() };
    assert!(interrupts.is_none(), "Interrupts already initialized!");

    // Create the GDT
    let (gdt, tss, tss_entry) = Gdt::new();

    // Create a pointer to the GDT for lgdt to load
    let gdt_ptr = TablePtr::new(
        core::mem::size_of_val(&gdt.raw[..]) as u16 - 1,
        gdt.raw.as_ptr() as u64);

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

    // Get the current long mode entry
    let (cs, _) = get_selector_indices();
    let cs = (cs * 8) as u16;

    // Create a new IDT
    let mut idt = Vec::with_capacity(INT_HANDLERS.len());
    for (id, &handler) in INT_HANDLERS.iter().enumerate().take(idt.capacity()) {
        let ist = match id {
            // NMI, #DF, #MC use the IST
            2 | 8 | 18 => { 1 },

            // The rest uses the existing stack
            _ => { 0 },
        };

        /// Interrupt gate type for 64-bit mode
        const X64_INTERRUPT_GATE: u32 = 0xE;

        // Construct the IDT entry pointing to the default handler
        idt.push(IdtEntry::new(
            cs,                 // Kernel code segment in the GDT
            handler as usize,   // Address of the handler for all interrupts
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
    ints.register_precedent(
        InterruptId::NonMaskableInterrupt, handler::nmi, false);
    ints.register_precedent(
        InterruptId::PageFault, handler::page_fault, false);

    *interrupts = Some(ints);
}

/// This is the entry point for all interrupts
#[unsafe(no_mangle)]
unsafe extern "sysv64" fn interrupt_entry(
    id: InterruptId,
    frame: &InterruptFrame,
    error: u64,
    regs: &AllRegs,
) {
    // Get the arguments for this interrupt
    let args = InterruptArgs::new(id, frame, error, regs);
    let idx = id as usize;

    // Increment the refcount for this interrupt. Gets decremented on scope end
    let _depth = if args.is_exception() {
        core!().enter_exception()
    } else {
        core!().enter_interrupt()
    };

    // Check whether we're draining interrupts and whether this interrupt gets
    // handled even during EOI draining
    let draining_eois = DRAINING_EOIS.load(Ordering::SeqCst);
    let precedent = DRAIN_PRECEDENCE[idx].load(Ordering::SeqCst);

    // If we're not draining interrupts, attempt to handle it
    let handled = if !draining_eois || precedent {
        unsafe {
            core!()
                .interrupts()
                .lock()
                .as_ref()
                .unwrap()
                .dispatch[idx]
                .is_some_and(|handler| handler(args))
        }
    } else {
        false
    };

    // EOI the APIC if required
    if EOI_REQUIRED[idx].load(Ordering::SeqCst) {
        unsafe { LocalApic::eoi() };

        // If we're only handling EOIs, we have handled what was requested
        if draining_eois { return; }
    }

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

    let id = args.id;
    let error = args.error;

    panic!(r#"
Unhandled interrupt <{id:X?}>, error code <{error:#X}> on core <{core_id}>
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

/// Legacy ISA interrupt identifiers
#[derive(Debug, PartialOrd, Ord, PartialEq, Eq, Copy, Clone)]
#[repr(u8)]
pub enum InterruptId {
    DivideBy0 = 0x00,
    // Reserved = 0x01,
    NonMaskableInterrupt = 0x02,
    Breakpoint,
    Overflow,
    BoundsRangeExceeded,
    InvalidOpcode,
    DeviceNotAvailable,
    DoubleFault,
    CoprocessorSegmentOverrun,
    InvalidTSS,
    SegmentNotPresent,
    StackSegmentFault,
    GeneralProtectionFault,
    PageFault,
    // Reserved = 0x0F,
    X87FPUError = 0x10,
    AlignmentCheck,
    MachineCheck,
    SIMDFloatingPointException,

    // Reserved hole
    Reserved = 0x14,
    LastReserved = 0x1F,

    // Kernel definable interrupts start at 0x20
    SoftRebootTimer = 0x20,
}

impl From<u8> for InterruptId {
    fn from(val: u8) -> Self {
        match val {
            // Well defined IDT entries
            0x00 => Self::DivideBy0,
            0x02 => Self::NonMaskableInterrupt,
            0x03 => Self::Breakpoint,
            0x04 => Self::Overflow,
            0x05 => Self::BoundsRangeExceeded,
            0x06 => Self::InvalidOpcode,
            0x07 => Self::DeviceNotAvailable,
            0x08 => Self::DoubleFault,
            0x09 => Self::CoprocessorSegmentOverrun,
            0x0A => Self::InvalidTSS,
            0x0B => Self::SegmentNotPresent,
            0x0C => Self::StackSegmentFault,
            0x0D => Self::GeneralProtectionFault,
            0x0E => Self::PageFault,
            0x10 => Self::X87FPUError,
            0x11 => Self::AlignmentCheck,
            0x12 => Self::MachineCheck,
            0x13 => Self::SIMDFloatingPointException,

            // Kernel defined IDT entries
            0x20 => Self::SoftRebootTimer,

            // Everything else is reserved and must not be used
            _ => Self::Reserved,
        }
    }
}

impl From<InterruptId> for usize {
    fn from(val: InterruptId) -> Self {
        val as u8 as usize
    }
}
