//! Local APIC implementation with support for xAPIC and x2APIC

#![allow(dead_code)]

use core::sync::atomic::Ordering;

use const_assert::const_assert;
use page_table::{
    PageType, PAGE_NXE, PAGE_WRITE, PAGE_CACHE_DISABLE, PAGE_PRESENT};

use crate::interrupts::InterruptId;

/// The x2APIC enable bit in the `IA32_APIC_BASE` MSR
const IA32_APIC_BASE_EXTD: u64 = 1 << 10;

/// The global enable bit in the `IA32_APIC_BASE` MSR
const IA32_APIC_BASE_EN: u64 = 1 << 11;

/// The intel specified APIC MSR
const IA32_APIC_BASE: u32 = 0x1B;

/// The physical address we want the local APIC to be mapped at. This should be
/// the standard base unless someone relocated it..
const APIC_BASE: u64 = 0xFEE0_0000;

// Validate the APIC base at compile time
const_assert!(
    APIC_BASE > 0 && APIC_BASE == (APIC_BASE & 0x0000_000f_ffff_f000));

/// Local APIC
pub struct LocalApic {
    /// The current operating mode of the APIC
    mode: ApicMode,

    /// The original state of the APIC pre-initialization
    orig: OrigState,
}

impl LocalApic {
    /// Get the APIC ID of the current running core
    pub fn id(&self) -> u32 {
        let apic_id = unsafe { self.read(Register::Id) };

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
                                              val as u32);
                }
                ApicMode::X2Apic => {
                    // Write the entire 64-bit value in one shot
                    cpu::wrmsr(0x830, val);
                }
            }
        }
    }

    /// Signal the end of an interrupt
    pub unsafe fn eoi() {
        // This EOI implementation must be completely lock-free because the APIC
        // might be accesses in NMIs or during a panic. The EOI wrmsr is atomic
        // with respect to other interrupts, so issuing it is safe.

        let apic = unsafe { &mut *core!().apic().shatter() };

        if let Some(apic) = apic {
            unsafe { apic.write(Register::EndOfInterrupt, 0); }
        }
    }

    /// Returns the 256-bits of in-service register state
    /// Array is [low 128 bits, high 128 bits]
    pub unsafe fn isr(&self) -> [u128; 2] {
        // Storage for the 256-bits of data
        let mut isr: [u8; 32] = [0; 32];

        // Values to load
        let to_load = [
            Register::Isr0, Register::Isr1, Register::Isr2, Register::Isr3,
            Register::Isr4, Register::Isr5, Register::Isr6, Register::Isr7,
        ];

        // Read all the registers into `isr`
        for (ii, &reg) in to_load.iter().enumerate() {
            unsafe {
                isr[ii * size_of::<u32>()..(ii + 1) * size_of::<u32>()]
                    .copy_from_slice(&self.read(reg).to_le_bytes());
            }
        }

        // Turn the 32 `u8`s into 2 `u128`s
        [
            u128::from_le_bytes(isr[..16].try_into().unwrap()),
            u128::from_le_bytes(isr[16..].try_into().unwrap()),
        ]
    }

    /// Returns the 256-bits of interrupt request register state
    /// Array is [low 128 bits, high 128 bits]
    pub unsafe fn irr(&self) -> [u128; 2] {
        // Storage for the 256-bits of data
        let mut irr: [u8; 32] = [0; 32];

        // Values to load
        let to_load = [
            Register::Irr0, Register::Irr1, Register::Irr2, Register::Irr3,
            Register::Irr4, Register::Irr5, Register::Irr6, Register::Irr7,
        ];

        // Read all the registers into `irr`
        for (ii, &reg) in to_load.iter().enumerate() {
            unsafe {
                irr[ii * size_of::<u32>()..(ii + 1) * size_of::<u32>()]
                    .copy_from_slice(&self.read(reg).to_le_bytes());
            }
        }

        // Turn the 32 `u8`s into 2 `u128`s
        [
            u128::from_le_bytes(irr[..16].try_into().unwrap()),
            u128::from_le_bytes(irr[16..].try_into().unwrap()),
        ]
    }

    /// Reset the APIC to the original state before we took control of it.
    pub unsafe fn reset(&mut self) {
        const LVT_MASK: u32 = 1 << 16;

        // Just about everything this function does is unsafe..
        unsafe {
        // Disable timer interrupts by masking them off
        self.write(Register::LvtTimer,
            self.read(Register::LvtTimer) | LVT_MASK);

        // If there are any timer interrupts in service, EOI them as we're
        // tearing the APIC down
        loop {
            let isr = self.isr();

            // If there are no interrupts being serviced, stop
            if isr[0] == 0 && isr[1] == 0 { break; }

            // EOI the interrupt
            Self::eoi();
        }

        // From now on we'll be draining interrupts instead of handling them.
        // Interrupts with draining precedence will still be handled
        crate::interrupts::DRAINING_EOIS.store(true, Ordering::SeqCst);

        // At this point the APIC is software disabled. If there are still any
        // interrupts requested that require EOI (as APIC interrupts do),
        // attempt to handle them
        loop {
            // Get the bitmasks of pending interrupts and those that require EOI
            let irr = self.irr();
            let eoi = crate::interrupts::eoi_bitmask();

            // Check if there are any pending
            let pending = (irr[0] & eoi[0]) != 0 || (irr[1] & eoi[1]) != 0;

            // Handle them if needed
            if pending {
                cpu::enable_interrupts()
            } else {
                break;
            }
        }

        // Disable interrupts and stop handling them
        cpu::disable_interrupts();

        // Restore the original APIC timer state
        {
            self.write(Register::DivideConfiguration, self.orig.timer.dcr);
            self.write(Register::LvtTimer, self.orig.timer.lvt);
            self.write(Register::InitialCount, self.orig.timer.icr);
        }

        // Load the original SVR
        self.write(Register::SpuriousInterruptVector, self.orig.svr);

        // Restore the original `IA32_APIC_BASE` to its original state.
        // Preserving the x2Apic because downgrading it in software may not be
        // supported.
        let apic_mode = if let ApicMode::X2Apic = self.mode {
            IA32_APIC_BASE_EXTD
        } else { 0 };
        cpu::wrmsr(IA32_APIC_BASE, self.orig.ia32_apic_base | apic_mode);

        // Reload the PIC's original state
        cpu::out8(0xA1, self.orig.pic_a1);
        cpu::out8(0x21, self.orig.pic_21);
        }
    }

    /// Enable the APIC timer which is used to check the serial port
    /// periodically to see if the user wants to issue a soft reboot
    pub unsafe fn enable_reboot_timer(&mut self) {
        const PERIODIC_MODE: u32 = 1 << 17;

        unsafe {
            // Set the initial count to 0, disabling the timer
            self.write(Register::InitialCount, 0);

            // Register an interrupt handler for this timer
            {
                core!().interrupts().lock().as_mut().unwrap().register(
                    InterruptId::SoftRebootTimer,
                    crate::interrupts::handler::soft_reboot_timer,
                    true);
            }

            // Set the timer divide register to divide by 2 (0 is correct)
            self.write(Register::DivideConfiguration, 0);

            // Program the APIC
            self.write(Register::LvtTimer,
                PERIODIC_MODE | (InterruptId::SoftRebootTimer as u8 as u32));

            // Enable the timer by setting the initial count
            self.write(Register::InitialCount, 100_000);
        }
    }
}

/// All of the stateful fields of the APIC timer
#[derive(Default)]
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
#[repr(u16)]
pub enum Register {
    /// APIC ID register
    Id = 0x20,

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
    // Don't reinitialize the APIC
    let mut cur_apic = core!().apic().lock();
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
        let orig_pic_a1 = cpu::in8(0xA1);
        let orig_pic_21 = cpu::in8(0x21);

        // Disable the old PIC by masking off all of its interrupts
        cpu::out8(0xA1, 0xFF);
        cpu::out8(0x21, 0xFF);

        // Reprogram the APIC with our new settings.
        cpu::wrmsr(IA32_APIC_BASE, apic_base);

        // Return out the original configuration
        (orig_ia32_apic_base, orig_pic_a1, orig_pic_21)
    };

    // If we're in normal xAPIC mode, map in the APIC physical memory
    let mode = if !cpu_features.x2apic {
        // Receive a virtual address for our mapping
        let vaddr = crate::mm::receive_vaddr_4k(PageType::Page4K as u64);

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
    let mut apic = LocalApic {
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
    core!().set_apic_id(apic.id());
    *cur_apic = Some(apic);
}
