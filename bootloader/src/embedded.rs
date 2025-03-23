//! This is where the embedded kernel image and trampoline will be present.
//!
//! On the first initial boot, the bootloader will use this image as the kernel
//! image which it will boot. On subsequent soft reboots, it is the kernel's
//! responsibility to download a fresh kernel image and to place it in a memory
//! location where the bootloader can access it such that it can boot the new
//! image instead of this one.

#[unsafe(no_mangle)]
#[unsafe(link_section = ".kernel")]
pub static INITIAL_KERNEL_IMAGE: &'static [u8] = include_bytes!(
    "../../kernel/target/kernel.bin");

#[unsafe(no_mangle)]
#[unsafe(link_section = ".trmpln")]
pub static TRAMPOLINE: &'static [u8] =
    include_bytes!("../target/trampoline.bin");

// Make sure the trampoline is too large
const_assert::const_assert!(
    (TRAMPOLINE.len() as u64) < shared_data::MAX_TRAMPOLINE_SIZE);
