//! Time and timing related routines

use core::sync::atomic::{Ordering, AtomicU64};

/// The TSC tick rate in MHz
///
/// This starts at a relatively sane default but will be set correctly by
/// `calibrate()`
static RDTSC_MHZ: AtomicU64 = AtomicU64::new(3_000);

/// TSC at the time of boot of the system
static RDTSC_START: AtomicU64 = AtomicU64::new(0);

#[inline]
/// Get the TSC rate in MHz
pub fn tsc_mhz() -> u64 {
    RDTSC_MHZ.load(Ordering::Relaxed)
}

#[inline]
/// Returns the TSC value upon a future time in microseconds
pub fn future(ms: u64) -> u64 {
    (unsafe { cpu::rdtsc() }) + (ms * tsc_mhz())
}

#[inline]
/// Busy sleep for a given number of microseconds
pub fn sleep(ms: u64) {
    let wait = future(ms);
    while (unsafe { cpu::rdtsc() }) < wait { core::hint::spin_loop(); }
}

/// Using the PIT, determine the frequency of rdtsc. Round this frequency to
/// the nearest 100MHz and return it.
pub unsafe fn calibrate() {
    // Store off the current rdtsc value
    let start = unsafe { cpu::rdtsc() };
    RDTSC_START.store(start, Ordering::Relaxed);

    // Start a timer
    let start = unsafe { cpu::rdtsc() };

    // Program the PIT to use mode 0 (interrupt after countdown) to
    // count down from 65535. This causes an interrupt to occur after
    // about 54.92 milliseconds (65535 / 1193182). We mask interrupts
    // from the PIT, thus we poll by sending the read back command
    // to check whether the output pin is set to 1, indicating the
    // countdown completed.
    unsafe {
        cpu::out8(0x43, 0x30);
        cpu::out8(0x40, 0xff);
        cpu::out8(0x40, 0xff);

        loop {
            // Send the read back command to latch status on channel 0
            cpu::out8(0x43, 0xe2);

            // If the output pin is high, then we know the countdown is
            // done. Break from the loop.
            if (cpu::in8(0x40) & 0x80) != 0 {
                break;
            }
        }
    }

    // Stop the timer
    let end = unsafe { cpu::rdtsc() };

    // Compute the time, in seconds, that the countdown was supposed to
    // take
    let elapsed: f64 = 65535. / 1193182.;

    // Compute MHz for the rdtsc
    let computed_rate = ((end - start) as f64) / elapsed / 1000000.0;

    // Round to the nearest 100MHz value
    let rounded_rate = (((computed_rate / 100.0) + 0.5) as u64) * 100;

    // Store the TSC rate
    RDTSC_MHZ.store(rounded_rate, Ordering::Relaxed);
}
