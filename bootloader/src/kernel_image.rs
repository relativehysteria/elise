//! This is where the embedded kernel image will be present.
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
