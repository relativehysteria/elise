//! UDP implementation

use alloc::sync::Arc;
use alloc::collections::VecDeque;

use crate::net::{ParseError, NetDevice, Port, Ip, Packet};

/// A parsed UDP header and payload
pub struct Udp<'a> {
    /// IP header
    pub ip: Ip<'a>,

    /// Destination port
    pub dst_port: Port,

    /// Source port
    pub src_port: Port,

    /// Raw byte payload of the UDP packet
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
}

impl Packet {
    /// Parse UDP information from the packet
    pub fn udp(&self) -> Result<Udp, ParseError> {
        // Parse the IP information header
        let ip = self.ip()?;

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

        Ok(Udp {
            payload: &ip.payload[8..length],
            src_port,
            dst_port,
            ip,
        })
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
}
