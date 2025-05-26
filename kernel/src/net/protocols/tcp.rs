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

/// Time in microseconds to wait before timing out on a SYN-ACK response
const TIMEOUT: u64 = 1_000_000;

/// Number of retries for a TCP connection rebind
const N_RETRIES: usize = 100_000;

/// TCP protocol for the IP header
const IP_PROT_TCP: u8 = 0x6;

// TODO: enum these

/// TCP synchronize flag (indicates a request to sync sequence numbers)
const TCP_SYN: u8 = 1 << 1;

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
    // TODO
    pub fn handle_packet(&mut self, _tcp: &Parsed) -> Option<usize> {
        None
    }
}

impl NetDevice {
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
                // MSS = 0x058C (1420)
                let opts = [2, 4, 0x5, 0x8C];
                let mut con = con.0.lock();
                let mut packet = dev.allocate_packet();
                {
                    packet.create_tcp(
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
        options: &'b [u8]
    ) -> Builder<'b> {
        Builder::from_packet(
            self.cursor(), addr, flags, seq, ack, window, options)
            .expect("Couldn't create a TCP packet")
    }
}
