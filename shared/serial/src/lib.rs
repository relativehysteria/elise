//! Serial 8250 UART driver

#![no_std]

use cpu;

/// Addresses of the legacy serial ports that are to be used by this driver
pub const PORT_ADDRESSES: [u16; 4] = [0x2F8, 0x3F8, 0x2E8, 0x3E8];

/// The serial driver implementation for COM ports defined by `PORT_ADDRESSES`
#[derive(Clone, Debug)]
pub struct SerialDriver {
    /// Index map of which ports in `PORT_ADDRESSES` are valid.
    pub ports: [Option<u16>; PORT_ADDRESSES.len()]
}

impl SerialDriver {
    /// Initialize the serial ports on the system to 28800n1. This should only
    /// ever be called once, therefore it is marked as unsafe.
    pub unsafe fn init() -> Self {
        // Create a new serial port driver
        let mut driver = Self {
            ports: [None; PORT_ADDRESSES.len()],
        };

        // Go through each defined port
        for (idx, &port) in PORT_ADDRESSES.iter().enumerate() {
            unsafe {
                // Disable all interrupts
                cpu::out8(port + 1, 0x00);

                // Enable DLAB (set baud divisor)
                cpu::out8(port + 3, 0x80);

                // Divisor = 115200 / divisor;
                // low byte and high byte of the divisor, respectively
                cpu::out8(port, 0x04);
                cpu::out8(port + 1, 0x00);

                // 8 bits, no parity, one stop bit
                cpu::out8(port + 3, 0x03);

                // IRQs disabled, RTS/DSR set
                cpu::out8(port + 4, 0x03);

                // Test the serial chip

                // Set it to loopback mode
                cpu::out8(port + 4, 0x1E);

                // Send a byte
                cpu::out8(port, 0xAE);

                // Check if the byte is returned back
                if cpu::in8(port) == 0xAE {
                    // It is -- set the port back to normal mode
                    cpu::out8(port + 4, 0x0F);

                    // Register the port
                    driver.ports[idx] = Some(port);
                }
            }
        }

        // Drain all the ports of inbound bytes
        while let Some(_) = driver.read_byte() {}
        driver
    }

    /// Read a byte from whatever port has a byte available
    pub fn read_byte(&mut self) -> Option<u8> {
        // Go through each port
        for &port in self.ports.iter().flatten() {
            unsafe {
                // Check if there is a byte available
                if (cpu::in8(port + 5) & 1) != 0 {
                    // Read the byte that was present on this port and
                    // return it
                    return Some(cpu::in8(port));
                }
            }
        }

        // No bytes available
        None
    }

    /// Write a byte to all ports available
    fn write_byte(&mut self, byte: u8) {
        // Write a CR before LF
        if byte == b'\n' { self.write_byte(b'\r'); }

        // Go through each port
        for &port in self.ports.iter().flatten() {
            // Wait for the transmit buffer to be ready
            unsafe {
                while (cpu::in8(port + 5) & 0x20) == 0 {
                    core::hint::spin_loop();
                }

                // Write the byte
                cpu::out8(port, byte);
            }
        }
    }

    /// Write bytes to all ports available
    pub fn write(&mut self, bytes: &[u8]) {
        // Broadcast each byte to all available ports
        bytes.iter().for_each(|&byte| self.write_byte(byte));
    }
}
