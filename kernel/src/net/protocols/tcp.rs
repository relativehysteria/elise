//! L3: TCP implementation

use alloc::collections::VecDeque;
use alloc::sync::Arc;
use core::net::Ipv4Addr;

use spinlock::SpinLock;

use crate::net::packet::{Packet, PacketCursor, ParseError};
use crate::net::protocols::{ip, eth};
use crate::net::{NetDevice, NetAddress, Port};
use crate::core_locals::InterruptLock;

/// Maximum number of bytes ot use for TCP windows
const WINDOW_SIZE: usize = u16::MAX as usize;

/// Time in microseconds to wait before timing out on ACK responses
const TIMEOUT: u64 = 1_000_000;

/// Number of retries for a TCP connection rebind
const N_RETRIES: usize = 100_000;

/// TCP protocol for the IP header
const IP_PROT_TCP: u8 = 0x6;

/// Maximum MSS the TCP stack will use
const MAX_MSS: usize = 1420;

// TODO: enum these

/// TCP synchronize flag (indicates a request to sync sequence numbers)
const TCP_SYN: u8 = 1 << 1;

/// TCP reset flag (resets a TCP connection)
const TCP_RST: u8 = 1 << 2;

/// TCP acknowledge
const TCP_ACK: u8 = 1 << 4;

/// A parsed TCP header and payload
#[derive(Debug)]
pub struct Parsed<'a> {
    /// IP header
    pub ip: ip::Parsed<'a>,

    /// Source port
    pub src_port: Port,

    /// Destination port
    pub dst_port: Port,

    /// Sequence identifier
    pub seq: u32,

    /// ACK number
    pub ack: u32,

    /// Size of the receive window
    pub window: u16,

    /// TCP flags
    pub flags: u8,

    /// Raw byte payload
    pub payload: &'a [u8],
}

/// The possible states of a TCP connection
#[derive(Debug, PartialEq, Eq)]
enum TcpState {
    /// The connection is closed
    Closed,

    /// An initial SYN has been sent and we are awating a SYN-ACK
    Syn,

    /// We have sent an ACK to a SYN-ACK, marking the connection as established.
    ///
    /// It's possible we might get another SYN-ACK in this state in the case
    /// that the ACK we have sent was dropped
    Established,
}

/// A TCP connection
pub struct Connection(SpinLock<Internal, InterruptLock>);

impl Connection {
    /// Send a payload over the TCP connection
    /// Returns `Some(())` if all bytes are sent. This function won't return
    /// until all bytes are sent (possibly may block forever).
    pub fn send(&self, buf: &[u8]) -> Option<()> {
        // ACK receive timeout. This is set to `!0` to avoid hiting it on the
        // first loop cycle, but will be set to a sane value later
        let mut timeout = !0;

        // Pointer to the data that we have yet to send
        let mut to_send = &buf[..];

        // Parse responses until we recieve a packet not destined to us
        loop {
            {
                // Get access to the connection
                let mut con = self.0.lock();
                if con.state != TcpState::Established { return None; }

                // If we didn't get an ACK before timing out, we either have no
                // window left, or we have sent everything and we're waiting for
                // the final ack
                //
                // Calculate how much was sent but unacknowledged
                if cpu::rdtsc() >= timeout {
                    // Compute how much we have sent so far
                    let sent = buf.len() - to_send.len();

                    // Compute the number of unacked bytes
                    let unacked = con.seq.wrapping_sub(con.remote_ack) as usize;

                    // Rewind the pointer
                    to_send = &buf[sent - unacked..];

                    // Reset the seq
                    con.seq = con.remote_ack;
                }

                // If everything is sent and acked, return
                if to_send.len == 0 && con.remote_ack = con.seq {
                    return Some(());
                }

                // Check if we got an ACK and handle it
                if let Some(pkt) = dev.as_ref().unwrap().recv() {
                    if let Some(tcp) = pkt.parse_tcp() {
                        if tcp.dst_port != con.port {
                            // Packet not for us
                            core::mem::drop(con);
                            dev.as_ref().unwrap().discard(pkt);
                            continue;
                        }

                        // Handle the packet that is confirmed destined to us
                        con.handle_packet(&tcp);
                    } else {
                        // Not TCP; discard it
                        core::mem::drop(con);
                        dev.as_ref().unwrap().discard(pkt);
                    }
                }
            }

            // Get mut access to the connection
            let mut con = self.0.lock();

            // Cap the MSS
            let mss = core:;cmp::min(con.remote_mss as usize, MAX_MSS);

            // Compute the number of unacknowledged bytes
            let unacked = con.seq.wrapping_sub(con.remote_ack) as usize;

            // Compute the number of bytes the remote server is capable of
            // accepting
            let remaining = core::cmp::min(
                to_send.len(),
                (con.remote_window as usize).saturating_sub(unacked));

            // Everything sent; wait for the final ack
            if remain == 0 { continue; }

            // Send the MSS-sized chunks of the buffer
            let mut iter = to_send[..remaining].chunks(mss);
            while let Some(chunk) = iter.next() {
                // Create the packet
                let mut packet = dev.as_ref().unwrap().allocate_packet();
                {
                    let mut cursor = packet.create_tcp(
                        &con.server,
                        TCP_ACK | if iter.len() == 0 { TCP_PSH } else { 0 },
                        con.seq,
                        con.ack,
                        (WINDOW_SIZE - con.window.len()) as u16);
                    cursor.write(chunk);
                }

                // Update our seq and send the packet
                con.seq = con.seq.wrapping_add(chunk.len() as u32);
                dev.as_ref().unwrap().send(packet, iter.len() == 0);
            }

            // Advance the pointer reflecting what we sent
            to_send = &to_send[remain..];

            // Set a timeout for a window update
            timout = crate::time::future(1_000);
        }
    }

    /// Receives data from the TCP connection
    pub fn recv(&self, buf: &mut[u8]) -> Option<usize> {
        // Get a pointer to the buffer
        let mut to_recv = &mut buf[..];

        // Get mutable access to the TCP connection
        let mut con = self.0.lock();

        // If the connection isn't established, nothing to do
        if con.state != TcpState::Established { return None; }

        // TODO
    }
}

/// The actual internal state of a TCP connection
#[allow(dead_code)]
pub struct Internal {
    /// TCP receive window
    window: VecDeque<u8>,

    /// The network device this connection is bound on
    dev: Arc<NetDevice>,

    /// Address of the remote server
    server: NetAddress,

    /// State of the connection
    state: TcpState,

    /// The port we are bound on
    port: Port,

    /// The connection sequence identifier
    seq: u32,
}

impl Internal {
    /// Handle a TCP packet
    ///
    /// This could be _any_ TCP packet
    pub fn handle_packet(&mut self, tcp: &Parsed) -> Option<()> {
        // Packets not destined to us should be discarded by the caller
        assert!(self.port == tcp.dst_port,
            "Packets not destined to `handle_packet()` should be discarded");

        // Don't handle packets if the connection is closed
        if self.state == TcpState::Closed { return None; }

        // If we got a reset, close the connection
        // TODO: handle FINs and close the connection gracefully
        if tcp.flags & TCP_RST != 0 {
            self.state = TcpState::Closed;
            return None;
        }

        // At this point any point we only expect ACKs
        if tcp.flags & TCP_ACK == 0 { return None; }

        // Get the number of unacknowledged bytes
        let unacked = self.seq.wrapping_sub(self.remote_ack);

        // Make sure the remote end is not acknowledging bytes we never sent
        if tcp.ack.wrapping_sub(self.remote_ack) > unacked { return None; }

        // TODO: handle out of order packets
        // For now, we'll just drop them
        if self.state == TcpState::Established && tcp.seq != seq.ack {
            return None;
        }

        // Track whether we need to send an ACK
        let mut should_ack = false;

        // Check if the packet contains any data and if it does, copy it to our
        // window
        if self.state == TcpState::Established && tcp.payload.len() > 0 {
            // Drop packets that exceed our window; the remote side should never
            // send more than that.
            if tcp.payload.len() > WINDOW_SIZE - self.window.len() {
                return None;
            }

            // Save the data into oru window
            self.window.extend(tcp.payload);

            // Update the ack to indicate we read the bytes
            self.ack = self.ack.wrapping_add(tcp.payload.len() as u32);
            should_ack = true;
        }

        // If we're waiting for a SYN-ACK, check if this is it
        if (self.state == Tcp::SynSent || self.state == TcpState::Established)
                && tcp.flags & TCP_SYN != 0 {
            // If we just acked a SYN, update the state
            self.state = TcpState::Established;
            self.ack = tcp.seq.wrapping_add(1);
            should_ack = true;
        }

        // Send an ACK if needed
        if should_ack {
            let mut packet = self.dev.allocate_packet();
            {
                packet.create_tcp(
                    &self.server, TCP_ACK, self.seq, self.ack,
                    (WINDOW_SIZE as usize - self.window.len()) as u16);
            }
            self.dev.send(packet, true);
        }

        // Update the server state to the most recent packet information
        self.remote_ack = tcp.ack;
        self.remote_window = tcp.window;
        Some(())
    }
}

impl NetDevice {
    /// Discard a TCP packet and attempt to handle it somewhere else in the
    /// network stac.
    ///
    /// If this function handles the packet, it will be taken out of the option
    pub fn discard_tcp(&self, packet: &mut Option<PacketLease>) {
        let pk = match packet.take() {
            None => return,
            Some(pk) = pk,
        };

        // Parse the packet as TCP
        let tcp = match pk.parse_tcp() {
            Ok(tcp) => tcp,
            _ => {
                // Couldn't parse as TCP. put the packet back and return
                *packet = Some(pkt);
                return;
            }
        };

        // Get access to TCP connections
        let mut cons = self.tcp_connections.lock();

        // If we have a connection for this port, attempt to handle the packet
        if let Some(con) = cons.get_mut(&tcp.dst_port) {
            let con = con.clone();
            core::mem::drop(cons);
            con.lock().handle_packet(&tcp);
            return;
        }
    }

    /// Create a connection to the remote server specified by the IP and port
    pub fn tcp_connect(dev: Arc<NetDevice>, dst_ip: Ipv4Addr, dst_port: Port)
            -> Option<Arc<Connection>> {
        // Bind/rebind a TCP connection on the first free port
        'rebind: for _ in 0..N_RETRIES {
            // Acquire a possibly unbound port and resolve the server address
            let port = Port::next_free();
            let server = NetAddress::resolve(&dev, port, dst_port, dst_ip)?;

            // Attempt to create and register a connection with this port
            let con = {
                // Check if the port is reserved
                let mut cons = dev.tcp_connections.lock();
                if cons.contains_key(&port) { continue; }

                // Port not reserved yet. Create a TCP connection
                let seq = cpu::rdtsc() as u32;
                let con = SpinLock::new(Internal {
                    window: VecDeque::with_capacity(WINDOW_SIZE),
                    dev:    dev.clone(),
                    state:  TcpState::Closed,
                    server,
                    port,
                    seq,
                });
                let con = Arc::new(Connection(con));

                // Save the connection
                cons.insert(port, con.clone());
                con
            };

            // Send a SYN packet
            {
                let opts = [2, 4, (MAX_MSS >> 8) as u8, (MAX_MSS & 0xFF) as u8];
                let mut con = con.0.lock();
                let mut packet = dev.allocate_packet();
                {
                    packet.create_tcp_options(
                        &con.server,
                        TCP_SYN,
                        con.seq,
                        0,
                        (WINDOW_SIZE - con.window.len()) as u16,
                        &opts);
                }
                // Send the packet, update the seq number and the TCP state
                dev.send(packet, true);
                con.seq = con.seq.wrapping_add(1);
                con.state = TcpState::Syn;
            }

            // Wait for SYN-ACK with timeout
            let timeout = crate::time::future(TIMEOUT);
            loop {
                // Check connection state first. If the connection has been
                // established, stop, otherwise cleanup on timeout and retry
                {
                    let con = con.0.lock();
                    if con.state == TcpState::Established { break; }
                    if cpu::rdtsc() >= timeout {
                        dev.tcp_connections.lock().remove(&port);
                        continue 'rebind;
                    }
                }

                // Process incoming packets
                if let Some(pkt) = dev.recv() {
                    // Parse the packet as TCP
                    let tcp = match pkt.parse_tcp() {
                        Ok(tcp) => tcp,
                        Err(_) => {
                            dev.discard(pkt);
                            continue;
                        }
                    };

                    // If we have a TCP packet for a differnet port, discard it
                    if tcp.dst_port != port {
                        dev.discard(pkt);
                        continue;
                    }

                    // Packet for us; handle it
                    let mut con = con.0.lock();
                    con.handle_packet(&tcp);
                }
            }

            return Some(con);
        }

        // Could not get a connection
        // Resolve the
        None
    }
}

impl<'a> ip::Builder<'a> {
    /// Creates a new TCP builder out of this IP builder
    pub fn tcp(
        mut self,
        src: &'a Port,
        dst: &'a Port,
        flags: u8,
        seq: u32,
        ack: u32,
        window: u16,
        opts: &'a [u8]
    ) -> Option<Builder<'a>> {
        // Set the protocol
        self.set_protocol(ip::TransportProtocol::Tcp);

        // Take out the cursor as we're no longer gonna need it
        let cursor = self.take_cursor().unwrap();

        Builder::new(self, cursor, src, dst, flags, seq, ack, window, opts)
    }
}

/// Indexes of TCP fields which have to be filled in when the packet is
/// finalized
struct ToFill {
    crc: usize,
}

/// Builder for TCP packets
pub struct Builder<'a> {
    pub(super) ip:      ip::Builder<'a>,
    pub(super) hdr:     &'a mut [u8],
    pub(super) payload: PacketCursor<'a>,
    to_fill: ToFill,
}

impl<'a> Builder<'a> {
    /// Creates a new TCP builder
    pub fn new(
        mut ip: ip::Builder<'a>,
        mut cursor: PacketCursor<'a>,
        src: &'a Port,
        dst: &'a Port,
        flags: u8,
        seq: u32,
        ack: u32,
        window: u16,
        opts: &'a [u8]
    ) -> Option<Self> {
        // Set the protocol
        ip.set_protocol(ip::TransportProtocol::Tcp);

        // Write down everything as it was given to us
        cursor.write_u16(src.0)?;
        cursor.write_u16(dst.0)?;
        cursor.write_u32(seq)?;
        cursor.write_u32(ack)?;
        let (data_offset, _) = cursor.write_u8(0)?;
        cursor.write_u8(flags)?;
        cursor.write_u16(window)?;
        let (crc, _) = cursor.write_u16(0)?;
        cursor.write_u8(0)?; // Urgent pointer
        cursor.write(opts)?;

        // Split the header and the payload
        let (hdr, payload) = cursor.split_at_current();

        // Set the data offset
        let offset = (hdr.len() / 4) as u8;
        assert!(offset <= 15, "Too many options provided to TCP builder");
        hdr[data_offset] = offset << 4;

        // Save off the indexes of fields which we'll edit later
        let to_fill = ToFill { crc };

        Some(Self { ip, hdr, payload, to_fill })
    }

    /// Create a new TCP builder from the provided packet
    pub fn from_packet(
        cursor: PacketCursor<'a>,
        addr: &'a NetAddress,
        flags: u8,
        seq: u32,
        ack: u32,
        window: u16,
        opts: &'a [u8],
    ) -> Option<Self> {
        eth::Builder::new(cursor, &addr.src_mac, &addr.dst_mac)?
            .ip(&addr.src_ip, &addr.dst_ip)?
            .tcp(&addr.src_port, &addr.dst_port, flags, seq, ack, window, opts)
    }

    /// Calculates and writes the CRC
    fn write_crc(&mut self) {
        // Determine IP version and gather information for pseudo-header
        let (src_ip, dst_ip, len, is_ipv4) = match &self.ip {
            ip::Builder::V4(ipv4) => {
                let len = (self.hdr.len() + self.payload.get().len()) as u32;
                (
                    &ipv4.src().octets()[..],
                    &ipv4.dst().octets()[..],
                    len,
                    true,
                )
            }
            ip::Builder::V6(ipv6) => {
                let len = (self.hdr.len() + self.payload.get().len()) as u32;
                (
                    &ipv6.src().octets()[..],
                    &ipv6.dst().octets()[..],
                    len,
                    false,
                )
            }
        };

        // Build the appropriate pseudo-header based on IP version
        let mut ph = [0u8; 40];
        let pseudo_header = if is_ipv4 {
            // IPv4 pseudo-header (12 bytes)
            ph[0..4].copy_from_slice(&src_ip);
            ph[4..8].copy_from_slice(&dst_ip);
            ph[9] = IP_PROT_TCP;
            ph[10..12].copy_from_slice(&(len as u16).to_be_bytes());
            &ph[0..12]
        } else {
            // IPv6 pseudo-header (40 bytes)
            ph[0..16].copy_from_slice(&src_ip);
            ph[16..32].copy_from_slice(&dst_ip);
            ph[32..36].copy_from_slice(&len.to_be_bytes());
            ph[39] = IP_PROT_TCP;
            &ph[0..40]
        };

        // Calculate checksum accumulator
        let mut acc: u32 = 0;
        acc = acc.wrapping_add(Packet::checksum(pseudo_header) as u32);
        acc = acc.wrapping_add(Packet::checksum(&self.hdr) as u32);
        acc = acc.wrapping_add(Packet::checksum(self.payload.get()) as u32);

        // Fold and complement to get final checksum
        let mut final_sum = (acc & 0xFFFF) + (acc >> 16);
        final_sum = (final_sum & 0xFFFF) + (final_sum >> 16);
        let checksum = !(final_sum as u16);
        let checksum = if checksum == 0 { 0xFFFF } else { checksum };

        // Write checksum to TCP header
        let idx = self.to_fill.crc;
        self.hdr[idx..idx + 2].copy_from_slice(&checksum.to_be_bytes());
    }

    /// Finalizes the TCP packet, writing in the checksum
    pub fn finalize(&mut self) {
        // Get the size of the header and the payload and write it down
        let tcp_len = (self.hdr.len() + self.payload.get().len()) as u16;
        self.ip.finalize(tcp_len);

        // Calculate the CRC
        self.write_crc();
    }
}

impl<'a> Drop for Builder<'a> {
    fn drop(&mut self) {
        self.finalize()
    }
}

impl Packet {
    pub fn parse_tcp(&self) -> Result<Parsed, ParseError> {
        // Parse the IP information header
        let ip = self.parse_ipv4().map(ip::Parsed::V4)
            .or_else(|_| self.parse_ipv6().map(ip::Parsed::V6))?;

        // Minimum TCP header size in bytes
        let min_hdr = 20;

        // Check the protocol
        if ip.protocol() != IP_PROT_TCP {
            return Err(ParseError::InvalidIpProtocol);
        }

        // Check the length
        if ip.payload().len() < min_hdr {
            return Err(ParseError::InvalidLength);
        }

        // Make sure data offset makes sense
        let doffset = ((ip.payload()[0xC] >> 4) * 4) as usize;
        if doffset < min_hdr || doffset > ip.payload().len() {
            return Err(ParseError::InvalidLength);
        }

        // Parse the header
        let header = ip.payload().get(0..doffset)
            .ok_or(ParseError::TruncatedPacket)?;
        let payload = &ip.payload().get(doffset..)
            .ok_or(ParseError::TruncatedPacket)?;
        Ok(Parsed {
            src_port: Port(Packet::parse_u16(header.get(0..2))?),
            dst_port: Port(Packet::parse_u16(header.get(2..4))?),
            seq: Packet::parse_u32(header.get(4..8))?,
            ack: Packet::parse_u32(header.get(8..12))?,
            window: Packet::parse_u16(header.get(14..16))?,
            flags: header.get(13).copied().ok_or(ParseError::TruncatedPacket)?,
            payload,
            ip,
        })
    }

    /// Create a TCP builder out of this packet
    ///
    /// Panics if the builder can't be created
    pub fn create_tcp<'a: 'b, 'b>(
        &'a mut self,
        addr: &'b NetAddress,
        flags: u8,
        seq: u32,
        ack: u32,
        window: u16,
    ) -> Builder<'b> {
        self.create_tcp(addr, flags, seq, ack, window, &[])
    }

    /// Create a TCP builder out of this packet, setting `options` as TCP opts
    ///
    /// Panics if the builder can't be created
    pub fn create_tcp_options<'a: 'b, 'b>(
        &'a mut self,
        addr: &'b NetAddress,
        flags: u8,
        seq: u32,
        ack: u32,
        window: u16,
        options: &'b [u8]
    ) -> Builder<'b> {
        Builder::from_packet(
            self.cursor(), addr, flags, seq, ack, window, options)
            .expect("Couldn't create a TCP packet")
    }
}
