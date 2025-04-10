//! Local APIC implementation with support for xAPIC and x2APIC

#![allow(dead_code)]

use page_table::{
    PageType, PAGE_NXE, PAGE_WRITE, PAGE_CACHE_DISABLE, PAGE_PRESENT};

/// The x2APIC enable bit in the `IA32_APIC_BASE` MSR
const IA32_APIC_BASE_EXTD: u64 = 1 << 10;

/// The global enable bit in the `IA32_APIC_BASE` MSR
const IA32_APIC_BASE_EN: u64 = 1 << 11;

/// The intel specified APIC MSR
const IA32_APIC_BASE: u32 = 0x1B;

/// The physical address we want the local APIC to be mapped at. This should be
/// the standard base unless someone relocated it..
const APIC_BASE: u64 = 0xFEE0_0000;

/// Local APIC
pub struct Apic {
    /// The current operating mode of the APIC
    mode: ApicMode,

    /// The original state of the APIC pre-initialization
    orig: OrigState,
}

impl Apic {
    /// Get the APIC ID of the current running core
    pub fn id(&self) -> u32 {
        let apic_id = unsafe { self.read(Register::ApicId) };

        match &self.mode {
            ApicMode::Apic(_) => apic_id >> 24,
            ApicMode::X2Apic  => apic_id,
        }
    }

    /// Read a value from the given APIC `register`
    pub unsafe fn read(&self, register: Register) -> u32 {
        let offset = register as usize;

        unsafe {
            match &self.mode {
                ApicMode::Apic(mapping) => {
                    core::ptr::read_volatile(&mapping[offset / 4])
                },
                ApicMode::X2Apic => {
                    cpu::rdmsr(0x800 + (offset as u32 / 16)) as u32
                },
            }
        }
    }

    /// Write a `value` to the given APIC `register`
    pub unsafe fn write(&mut self, register: Register, value: u32) {
        let offset = register as usize;

        unsafe {
            match &mut self.mode {
                ApicMode::Apic(mapping) => {
                    core::ptr::write_volatile(&mut mapping[offset / 4], value);
                },
                ApicMode::X2Apic => {
                    cpu::wrmsr(0x800 + (offset as u32 / 16), value as u64);
                }
            }
        }
    }

    /// Send a raw inter-processor interrupt to a specific APIC ID
    /// It is up to the caller to make sure the `dest_id` is a valid APIC ID and
    /// the IPI is a valid IPI type/format.
    pub unsafe fn ipi(&mut self, dest_id: u32, ipi: u32) {
        // Convert the destination APIC ID into the correct location based on
        // the APIC mode
        let dest_id = match &self.mode {
            ApicMode::Apic(_) => dest_id << 24,
            ApicMode::X2Apic  => dest_id,
        };

        // Construct the IPI command and send it!
        unsafe { self.write_icr(((dest_id as u64) << 32) | ipi as u64); }
    }

    /// Write a value to the APIC's ICR
    unsafe fn write_icr(&mut self, val: u64) {
        unsafe {
            match &mut self.mode {
                ApicMode::Apic(mapping) => {
                    // Write the high part
                    core::ptr::write_volatile(&mut mapping[0x310 / 4],
                                              (val >> 32) as u32);

                    // Write the low part, causing the interrupt to be sent
                    core::ptr::write_volatile(&mut mapping[0x300 / 4],
                                              (val >>  0) as u32);
                }
                ApicMode::X2Apic => {
                    // Write the entire 64-bit value in one shot
                    cpu::wrmsr(0x830, val);
                }
            }
        }
    }
}

#[derive(Default)]
/// All of the stateful fields of the APIC timer
struct TimerState {
    /// Divide configuration register
    dcr: u32,

    /// Initial count register
    icr: u32,

    /// Timer LVT entry
    lvt: u32,
}

/// The original state of the local APIC before initialization
///
/// This state is used during soft reboots to bring the system to a sane default
struct OrigState {
    /// State of the `IA32_APIC_BASE`
    ia32_apic_base: u64,

    /// state of the SVR register (offset 0xF0)
    svr: u32,

    /// I/O port 0xA1 contents (PIC interrupt masks)
    pic_a1: u8,

    /// I/O port 0x21 contents (PIC interrupt masks)
    pic_21: u8,

    /// APIC timer state
    timer: TimerState,
}

/// APIC modes
enum ApicMode {
    /// Normal APIC mode
    Apic(&'static mut [u32]),

    /// APIC programmed to use x2apic mode
    X2Apic,
}

/// APIC registers (offsets into MMIO space)
#[derive(Clone, Copy)]
#[repr(usize)]
pub enum Register {
    /// APIC ID register
    ApicId = 0x20,

    /// End-of-interrupt register
    EndOfInterrupt = 0xb0,

    /// Spurious interrupt vector register (also has the software enable bits)
    SpuriousInterruptVector = 0xf0,

    /// In-Service Register bits 0..31
    Isr0 = 0x100,

    /// In-Service Register bits 32..63
    Isr1 = 0x110,

    /// In-Service Register bits 64..95
    Isr2 = 0x120,

    /// In-Service Register bits 96..127
    Isr3 = 0x130,

    /// In-Service Register bits 128..159
    Isr4 = 0x140,

    /// In-Service Register bits 160..191
    Isr5 = 0x150,

    /// In-Service Register bits 192..223
    Isr6 = 0x160,

    /// In-Service Register bits 224..255
    Isr7 = 0x170,

    /// Interrupt Request Register bits 0..31
    Irr0 = 0x200,

    /// Interrupt Request Register bits 32..63
    Irr1 = 0x210,

    /// Interrupt Request Register bits 64..95
    Irr2 = 0x220,

    /// Interrupt Request Register bits 96..127
    Irr3 = 0x230,

    /// Interrupt Request Register bits 128..159
    Irr4 = 0x240,

    /// Interrupt Request Register bits 160..191
    Irr5 = 0x250,

    /// Interrupt Request Register bits 192..223
    Irr6 = 0x260,

    /// Interrupt Request Register bits 224..255
    Irr7 = 0x270,

    /// LVT for the APIC timer
    LvtTimer = 0x320,

    /// APIC initial count register for APIC timer
    InitialCount = 0x380,

    /// APIC divide counter register for the APIC timer
    DivideConfiguration = 0x3E0,
}

/// Initialize and enable the local APIC for the current core.
///
/// If supported, will use x2APIC
pub unsafe fn init() {
    // Validate the APIC base
    assert!(APIC_BASE > 0 && APIC_BASE == (APIC_BASE & 0x0000_000f_ffff_f000),
            "Invalid APIC base address");

    // Don't reinitialize the APIC
    let mut cur_apic = unsafe { core!().apic().lock() };
    assert!(cur_apic.is_none(), "APIC already initialized!");

    // Get the CPU features
    let cpu_features = cpu::Features::get();

    // APIC must be supported
    assert!(cpu_features.apic, "APIC is not available on this system.");

    // Enable the APIC base
    let (orig_ia32_apic_base, orig_pic_a1, orig_pic_21) = unsafe {
        // Load the IA32_APIC_BASE
        let orig_ia32_apic_base = cpu::rdmsr(IA32_APIC_BASE);

        // The APIC must be globally enabled as re-enabling a disabled APIC is
        // not always supported.
        assert!((orig_ia32_apic_base & IA32_APIC_BASE_EN) != 0,
            "Enabling a globally disabled APIC is unsupported");

        // Mask off the old base address
        let apic_base = orig_ia32_apic_base & !0x0000_000F_FFFF_F000;

        // Put in the base we want to use and enable it
        let apic_base = apic_base | APIC_BASE | IA32_APIC_BASE_EN;

        // If supported, enable x2APIC
        let apic_base = apic_base
            | if cpu_features.x2apic { IA32_APIC_BASE_EXTD } else { 0 };

        // Save the old PIC state
        let orig_pic_a1 = cpu::in8(0xA1 as *const u8);
        let orig_pic_21 = cpu::in8(0x21 as *const u8);

        // Disable the old PIC by masking off all of its interrupts
        cpu::out8(0xA1 as *const u8, 0xFF);
        cpu::out8(0x21 as *const u8, 0xFF);

        // Reprogram the APIC with our new settings.
        cpu::wrmsr(IA32_APIC_BASE, apic_base);

        // Return out the original configuration
        (orig_ia32_apic_base, orig_pic_a1, orig_pic_21)
    };

    // If we're in normal xAPIC mode, map in the APIC physical memory
    let mode = if !cpu_features.x2apic {
        // Receive a virtual address for our mapping
        let vaddr = crate::mm::receive_vaddr_4k(4096);

        // Get access to the current page table
        let mut pmem = crate::mm::PhysicalMemory;
        let mut table = core!().shared.kernel_pt().lock();
        let table = table.as_mut().unwrap();

        let mapping = unsafe {
            table.map_raw(&mut pmem, vaddr, PageType::Page4K,
                    APIC_BASE | PAGE_NXE | PAGE_WRITE |
                    PAGE_CACHE_DISABLE | PAGE_PRESENT)
                .expect("Couldn't the APIC into virtual memory");

            // Convert the memory into a rust slice
            core::slice::from_raw_parts_mut(vaddr.0 as *mut u32, 1024)
        };

        ApicMode::Apic(mapping)
    } else {
        ApicMode::X2Apic
    };

    // Initialize the APIC struct
    let mut apic = Apic {
        mode,
        orig: OrigState {
            ia32_apic_base: orig_ia32_apic_base,
            pic_a1: orig_pic_a1,
            pic_21: orig_pic_21,
            svr: 0,
            timer: Default::default(),
        }
    };

    // Save the original SVR
    apic.orig.svr = unsafe { apic.read(Register::SpuriousInterruptVector) };

    // Save the original timer state
    apic.orig.timer = TimerState {
        dcr: unsafe { apic.read(Register::DivideConfiguration) },
        icr: unsafe { apic.read(Register::InitialCount) },
        lvt: unsafe { apic.read(Register::LvtTimer) },
    };

    // Enable the APIC, set spurious interrupt vector to 0xFF
    unsafe {
        apic.write(Register::SpuriousInterruptVector, (1 << 8) | 0xFF);
    }

    // Set the core's APIC id and reference
    unsafe { core!().set_apic_id(apic.id()); }
    *cur_apic = Some(apic);
}
