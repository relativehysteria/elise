//! L3: UDP implementation

use alloc::sync::Arc;
use alloc::collections::VecDeque;

use crate::net::packet::{Packet, PacketCursor, PacketLease, ParseError};
use crate::net::{NetDevice, Port, NetAddress};
use crate::net::protocols::{ip, eth};

/// UDP protocol for the IP header
const IP_PROT_UDP: u8 = 0x11;

/// A parsed UDP header and payload
#[derive(Debug)]
pub struct Parsed<'a> {
    /// IP header
    pub ip: ip::Parsed<'a>,

    /// Source port
    pub src_port: Port,

    /// Destination port
    pub dst_port: Port,

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
    pub fn recv_timeout<T, F>(&self, timeout: u64, mut func: F) -> Option<T>
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
                let ret = func(&packet, packet.parse_udp().unwrap());
                self.driver().release_packet(packet);
                return ret
            }
        }

        // No packet in the queue. Attempt to recv a new raw packet
        let packet = self.recv()?;

        // Attempt to parse the packet as UDP
        if let Ok(udp) = packet.parse_udp() {
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

    /// Discard a UDP packet and attempt to handle it somewhere else in the
    /// network stack.
    ///
    /// If this function handles the packet, it will be take out of the option
    pub fn discard_udp(&self, packet: &mut Option<PacketLease>) {
        let pk = match packet.take() {
            None => return,
            Some(pk) => pk,
        };

        // Parse the packet as UDP
        let udp = match pk.parse_udp() {
            Ok(udp) => udp,
            _ => {
                // Couldn't parse it as UDP. Put the packet back and return
                *packet = Some(pk);
                return;
            },
        };

        // Check if we are bound on this packet's port
        let mut binds = self.udp_binds.lock();
        let bind = match binds.get_mut(&udp.dst_port) {
            Some(b) => b,
            None    => return,
        };

        // We are bound on the packet's port, put it into queue if there's
        // space, otherwise drop it
        if bind.len() < bind.capacity() {
            let packet = packet.take().unwrap();
            bind.push_back(PacketLease::take(packet));
        } else {
            // Drop
        }
    }
}

impl Packet {
    /// Parse UDP information from the packet
    pub fn parse_udp(&self) -> Result<Parsed, ParseError> {
        // Parse the IP information header
        let ip = self.parse_ipv4().map(ip::Parsed::V4)
            .or_else(|_| self.parse_ipv6().map(ip::Parsed::V6))?;

        // Check that we're parsing a UDP packet
        if ip.protocol() != IP_PROT_UDP {
            return Err(ParseError::InvalidIpProtocol);
        }

        // Parse the header
        let header = ip.payload().get(0..8).ok_or(ParseError::TruncatedPacket)?;
        let src_port = Port(Packet::parse_u16(header.get(0..2))?);
        let dst_port = Port(Packet::parse_u16(header.get(2..4))?);
        let length   = Packet::parse_u16(header.get(4..6))? as usize;

        // Validate the length
        if length < header.len() || length > ip.payload().len() {
            return Err(ParseError::InvalidLength);
        }

        Ok(Parsed {
            payload: &ip.payload()[8..length],
            src_port,
            dst_port,
            ip,
        })
    }

    /// Create a UDP packet builder out of this packet
    ///
    /// Panics if the builder can't be created
    pub fn create_udp<'a: 'b, 'b>(&'a mut self, addr: &'b NetAddress)
            -> Builder<'b> {
        Builder::from_packet(self.cursor(), addr)
            .expect("Couldn't create a UDP packet")
    }
}

impl<'a> ip::Builder<'a> {
    /// Creates a new UDP builder out of this IP builder
    pub fn udp(mut self, src: &'a Port, dst: &'a Port) -> Option<Builder<'a>> {
        // Set the protocol
        self.set_protocol(ip::TransportProtocol::Udp);

        // Take out the cursor as we're no longer gonna need it
        let cursor = self.take_cursor().unwrap();

        Builder::new(self, cursor, src, dst)
    }
}

/// Indexes of the UDP fields which have to be filled in when the packet is
/// finalized
struct ToFill {
    len: usize,
    crc: usize,
}

/// Builder for UDP packets
pub struct Builder<'a> {
    pub(super) ip:      ip::Builder<'a>,
    pub(super) hdr:     &'a mut [u8],
    pub(super) payload: PacketCursor<'a>,
    to_fill: ToFill,
}

impl<'a> Builder<'a> {
    /// Creates a new UDP builder
    pub fn new(
        mut ip: ip::Builder<'a>,
        mut cursor: PacketCursor<'a>,
        src: &'a Port,
        dst: &'a Port,
    ) -> Option<Self> {
        // Set the protocol
        ip.set_protocol(ip::TransportProtocol::Udp);

        // Write down the ports
        cursor.write_u16(src.0)?;
        cursor.write_u16(dst.0)?;

        // Zero out the fields which have to be filled in later
        let (len, _) = cursor.write_u16(0)?;
        let (crc, _) = cursor.write_u16(0)?;

        // Split the header and the payload
        let (hdr, payload) = cursor.split_at_current();

        // Write down the fields that will have to be filled in later
        let to_fill = ToFill { len, crc };

        Some(Self { ip, hdr, payload, to_fill })
    }

    /// Creates a new UDP builder from this `cursor`
    pub fn from_packet(cursor: PacketCursor<'a>, addr: &'a NetAddress)
            -> Option<Self>{
        eth::Builder::new(cursor, &addr.src_mac, &addr.dst_mac)?
            .ip(&addr.src_ip, &addr.dst_ip)?
            .udp(&addr.src_port, &addr.dst_port)
    }

    /// Writes to the UDP payload if possible, as defined by the
    /// `Cursor::write()` spec
    pub fn write(&mut self, buf: &[u8]) -> Option<(usize, usize)> {
        self.payload.write(buf)
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

    /// Calculates and writes the CRC if the IP layer uses IPv6, otherwise keeps
    /// the CRC as 0, because IPv4 doesn't require it.
    fn write_crc(&mut self) {
        // IPv4 doesn't require a checksum
        let ip = match &self.ip {
            ip::Builder::V4(_) => return,
            ip::Builder::V6(ipv6) => ipv6,
        };

        // UDP length (header + payload)
        let udp_len = (self.hdr.len() + self.payload.get().len()) as u32;

        // Pseudo-header: source IP, dest IP, length, next header
        let mut pseudo_header = [0u8; 40];
        pseudo_header[00..16].copy_from_slice(&ip.src().octets());
        pseudo_header[16..32].copy_from_slice(&ip.dst().octets());
        pseudo_header[32..36].copy_from_slice(&udp_len.to_be_bytes());
        pseudo_header[39] = IP_PROT_UDP;

        // Start with an empty checksum accumulator
        let mut acc: u32 = 0;
        acc = acc.wrapping_add(Packet::checksum(&pseudo_header) as u32);
        acc = acc.wrapping_add(Packet::checksum(self.hdr) as u32);
        acc = acc.wrapping_add(Packet::checksum(self.payload.get()) as u32);

        // Final fold and complement
        let mut final_sum = (acc & 0xFFFF) + (acc >> 16);
        final_sum = (final_sum & 0xFFFF) + (final_sum >> 16);
        let checksum = !(final_sum as u16);
        let checksum = if checksum == 0 { 0xFFFF } else { checksum };

        // Write the checksum into the header
        let idx = self.to_fill.crc;
        self.hdr[idx..idx + 2].copy_from_slice(&checksum.to_be_bytes());
    }

    /// Finalizes the UDP packet, writing in all checksums and lengths
    pub fn finalize(&mut self) {
        // Get and write the UDP size
        let udp_len = self.write_len();

        // Finalize the IP header
        self.ip.finalize(udp_len);

        // Calculate the CRC
        self.write_crc();
    }
}

impl<'a> Drop for Builder<'a> {
    fn drop(&mut self) {
        self.finalize()
    }
}
