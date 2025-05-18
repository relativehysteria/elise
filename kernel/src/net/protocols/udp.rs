//! UDP implementation

use alloc::sync::Arc;
use alloc::collections::VecDeque;

use crate::net::packet::{Packet, PacketCursor, PacketLease, ParseError};
use crate::net::{NetDevice, Port, NetAddress};
use crate::net::protocols::{ip, eth};

/// A parsed UDP header and payload
pub struct Parsed<'a> {
    /// IP header
    pub ip: ip::ParsedV4<'a>,

    /// Destination port
    pub dst_port: Port,

    /// Source port
    pub src_port: Port,

    /// Raw byte payload
    pub payload: &'a [u8],
}

/// A UDP bound port
pub struct UdpBind {
    /// Reference to the device that is bound on this port
    dev: Arc<NetDevice>,

    /// The port this device is bound to
    port: Port,
}

impl UdpBind {
    /// Get the port number this UDP bind is bound to
    pub fn port(&self) -> Port {
        self.port
    }

    /// Get the device that is bound to this port
    pub fn device(&self) -> &NetDevice {
        &*self.dev
    }

    /// Attempt to receive a UDP packet on the bound port
    pub fn recv<T, F>(&self, mut func: F) -> Option<T>
    where
        F: FnMut(&Packet, Parsed) -> Option<T>,
    {
        self.device().recv_udp(self.port, &mut func)
    }

    /// Attempts to receive a UDP packet on the bound port for `timeout` μs
    pub fn recv_timeout<T, F>(&self, mut func: F, timeout: u64) -> Option<T>
    where
        F: FnMut(&Packet, Parsed) -> Option<T>,
    {
        let timeout = crate::time::future(timeout);
        loop {
            // Return nothing on timeout
            if cpu::rdtsc() >= timeout { return None; }

            if let Some(val) = self.device().recv_udp(self.port, &mut func) {
                return Some(val)
            }
        }
    }
}

impl Drop for UdpBind {
    fn drop(&mut self) {
        self.dev.unbind_udp(self.port);
    }
}

impl NetDevice {
    /// Create a UDP bind to an unused dynamic port
    pub fn bind_udp(dev: Arc<Self>) -> Option<UdpBind> {
        // Bind to the first UDP port that is available
        loop {
            let port = Port::next_free();
            if let Some(bind) = Self::bind_udp_port(dev.clone(), port) {
                return Some(bind);
            }
        }
    }

    /// Create a UDP bind to `port`
    pub fn bind_udp_port(dev: Arc<Self>, port: Port) -> Option<UdpBind> {
        // Acquire unique access to the binds
        let mut udp_binds = dev.udp_binds.lock();

        // If this port is already bound, bail out
        if udp_binds.contains_key(&port) {
            return None;
        }

        udp_binds.insert(port, VecDeque::new());

        core::mem::drop(udp_binds);

        Some(UdpBind { dev, port, })
    }

    /// Unbind from a UDP port
    fn unbind_udp(&self, port: Port) {
        if let Some(queue) = self.udp_binds.lock().remove(&port) {
            queue.into_iter()
                .for_each(|packet| self.driver().release_packet(packet));
        }
    }


    /// Receive a UDP packet destined to `port`
    fn recv_udp<T, F>(&self, port: Port, func: &mut F) -> Option<T>
    where
        F: FnMut(&Packet, Parsed) -> Option<T>
    {
        // If there's a packet in the queue, return it
        {
            let mut binds = self.udp_binds.lock();

            let ent = binds.get_mut(&port)?;
            if !ent.is_empty() {
                let packet = ent.pop_front().unwrap();
                let ret = func(&packet, packet.udp().unwrap());
                self.driver().release_packet(packet);
                return ret
            }
        }

        // No packet in the queue. Attempt to recv a new raw packet
        let packet = self.recv()?;

        // Attempt to parse the packet as UDP
        if let Ok(udp) = packet.udp() {
            // If it was destined to our port, return it
            if udp.dst_port == port {
                func(&*packet, udp)
            } else {
                self.discard(packet);
                None
            }
        } else {
            self.discard(packet);
            None
        }
    }

    /// Discard a UDP packet which was unhandled by this device and must be
    /// handled by another device which is expecting it
    ///
    /// Returns whether the `packet` has been handled by this function
    pub fn discard_udp(&self, packet: PacketLease) -> bool {
        // Parse the packet as UDP
        let udp = match packet.udp() {
            Ok(udp) => udp,
            _       => return false,
        };

        // Check if we are bound on this packet's port
        let mut binds = self.udp_binds.lock();
        let bind = match binds.get_mut(&udp.dst_port) {
            Some(b) => b,
            None    => return false,
        };

        // We are bound on the packet's port, put it into queue if there's
        // space, otherwise drop it
        if bind.len() < bind.capacity() {
            bind.push_back(PacketLease::take(packet));
        } else {
            // Drop
        }

        return true;
    }
}

impl Packet {
    /// Parse UDP information from the packet
    pub fn udp(&self) -> Result<Parsed, ParseError> {
        // Parse the IP information header
        let ip = self.ipv4()?;

        /// UDP protocol for the IP header
        const IP_PROT_UDP: u8 = 0x11;

        // Check that we're parsing a UDP packet
        if ip.protocol != IP_PROT_UDP {
            return Err(ParseError::InvalidIpProtocol);
        }

        // Parse the header
        let header = ip.payload.get(0..8).ok_or(ParseError::TruncatedPacket)?;
        let src_port = Port(Packet::parse_u16(header.get(0..2))?);
        let dst_port = Port(Packet::parse_u16(header.get(2..4))?);
        let length   = Packet::parse_u16(header.get(4..6))? as usize;

        // Validate the length
        if length < 8 || length > ip.payload.len() {
            return Err(ParseError::InvalidLength);
        }

        Ok(Parsed {
            payload: &ip.payload[8..length],
            src_port,
            dst_port,
            ip,
        })
    }
}

impl<'a> ip::BuilderV4<'a> {
    /// Creates a new UDP builder from this IP builder
    pub fn udp(mut self, src: &'a Port, dst: &'a Port) -> Option<Builder<'a>> {
        // Fill in the protocol
        self.hdr[self.to_fill.prot] = ip::IpProtocol::Udp as u8;

        // Take out the cursor as we're no longer gonna need it
        let cursor = self.cursor.take().unwrap();

        Builder::new(self, cursor, src, dst)
    }
}

/// Indexes of the UDP fields which have to be filled in when the packet is
/// finalized
struct ToFill {
    len: usize,
    _crc: usize,
}

/// Builder for UDP headers
pub struct Builder<'a> {
    ip: ip::BuilderV4<'a>,
    hdr: &'a mut [u8],
    payload: PacketCursor<'a>,
    to_fill: ToFill,
}

impl<'a> Builder<'a> {
    /// Creates a new UDP builder
    pub fn new(
        ip: ip::BuilderV4<'a>,
        mut cursor: PacketCursor<'a>,
        src: &'a Port,
        dst: &'a Port,
    ) -> Option<Self> {
        // Write down the ports
        cursor.write(src.0.to_be_bytes().as_ref())?;
        cursor.write(dst.0.to_be_bytes().as_ref())?;

        // Zero out the fields which have to be filled in later
        let (len, _) = cursor.write_u16(0)?;
        let (_crc, _) = cursor.write_u16(0)?;

        // Split the header and the payload
        let (hdr, payload) = cursor.split_at_current();

        // Write down the fields that will have to be filled in later
        let to_fill = ToFill { len, _crc };

        Some(Self { ip, hdr, payload, to_fill })
    }

    /// Writes to the UDP payload if possible, as defined by the
    /// `Cursor::write()` spec
    pub fn write(&mut self, buf: &[u8]) -> Option<(usize, usize)> {
        self.payload.write(buf)
    }

    /// Creates a new UDP builder from this `cursor`
    pub fn from_packet(cursor: PacketCursor<'a>, addr: &'a NetAddress)
            -> Option<Self>{
        // Construct the UDP builder
        eth::Builder::new(cursor, &addr.src_mac, &addr.dst_mac)?
            .ipv4(&addr.src_ip, &addr.dst_ip)?
            .udp(&addr.src_port, &addr.dst_port)
    }

    /// Writes down the size of the header and the payload into the header, and
    /// returns the size
    fn write_len(&mut self) -> u16 {
        // Calculate the total UDP size
        let size = (self.hdr.len() + self.payload.get().len()) as u16;

        // Write it down
        let idx = self.to_fill.len;
        self.hdr[idx..idx + 2].copy_from_slice(&size.to_be_bytes());

        size
    }

    /// Finalizes the UDP packet, writing in all checksums and lengths
    pub fn finalize(&mut self) {
        // Get the UDP size
        let udp_len = self.write_len();

        // Finalize the IP header
        self.ip.finalize(udp_len);

        // CRC is not required for IPv4
    }
}

impl<'a> Drop for Builder<'a> {
    fn drop(&mut self) {
        self.finalize()
    }
}
