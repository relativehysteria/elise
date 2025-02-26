use core::fmt::Write;
use serial::SerialDriver;
use crate::SHARED;

/// Dummy struct that implements `Write` such that `print!()` can be used on it
pub struct Serial;

impl Write for Serial {
    fn write_str(&mut self, string: &str) -> core::fmt::Result {
        let mut serial = SHARED.serial.lock();
        if let Some(serial) = &mut *serial {
            serial.write(string.as_bytes());
        }
        Ok(())
    }
}

/// Dummy struct that implements `Write` such that `print_shatter!()` can be
/// used on it, printing to the serial ports while bypassing the serial lock.
pub struct SerialShatter;

impl Write for SerialShatter {
    fn write_str(&mut self, string: &str) -> core::fmt::Result {
        unsafe {
            let serial = SHARED.serial.shatter();
            if let Some(serial) = &mut *serial {
                serial.write(string.as_bytes());
            }
        }
        Ok(())
    }
}

/// Serial `print!()` support for the bootloader
#[macro_export] macro_rules! print {
    ($($arg:tt)*) => {
        let _ = <$crate::print::Serial as core::fmt::Write>::write_fmt(
            &mut $crate::print::Serial, format_args!($($arg)*));
    }
}

/// Serial `print_shatter!()` support for the bootloader
#[macro_export] macro_rules! print_shatter {
    ($($arg:tt)*) => {
        let _ = <$crate::print::SerialShatter as core::fmt::Write>::write_fmt(
            &mut $crate::print::SerialShatter, format_args!($($arg)*));
    }
}
