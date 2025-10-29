// crates/powerlink-rs-linux/src/lib.rs
#![cfg(target_os = "linux")]

use pnet::datalink::{self, Channel, NetworkInterface as PnetInterface};
use powerlink_rs::{
    hal::PowerlinkError,
    types::{C_SDO_EPL_PORT, IpAddress},
    NetworkInterface,
};
use std::io;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::{Arc, Mutex}; // Use Arc for shared UDP socket
use std::time::Duration;

pub struct LinuxPnetInterface {
    // Raw Ethernet handling (remains the same)
    tx_raw: Mutex<Box<dyn datalink::DataLinkSender>>,
    rx_raw: Mutex<Box<dyn datalink::DataLinkReceiver>>,
    pnet_iface: PnetInterface,
    node_id: u8,
    mac_address: [u8; 6],
    // UDP Handling (added)
    udp_socket: Arc<UdpSocket>, // Arc allows sharing the socket if needed later
    local_ip_address: IpAddress,
}

impl LinuxPnetInterface {
    pub fn new(interface_name: &str, node_id: u8) -> Result<Self, String> {
        let interface = datalink::interfaces()
            .into_iter()
            .find(|iface: &PnetInterface| iface.name == interface_name)
            .ok_or_else(|| format!("Interface '{}' not found", interface_name))?;

        let mac_address = interface.mac.ok_or("Interface has no MAC address")?.into();

        // --- Raw Ethernet Setup ---
        let config_raw = datalink::Config {
            read_timeout: Some(Duration::from_millis(1)), // Use a small timeout for potentially non-blocking receive
            promiscuous: true,
            ..Default::default()
        };
        let (tx_raw, rx_raw) = match datalink::channel(&interface, config_raw) {
            Ok(Channel::Ethernet(tx, rx)) => (tx, rx),
            Ok(_) => return Err("Unsupported raw channel type".to_string()),
            Err(e) => return Err(format!("Raw channel error: {}", e)),
        };

        // --- UDP Socket Setup ---
        // Find the correct IPv4 address for the interface
        let local_ip = interface
            .ips
            .iter()
            .find(|ip_net| ip_net.is_ipv4())
            .map(|ip_net| match ip_net.ip() {
                std::net::IpAddr::V4(ipv4) => ipv4,
                _ => unreachable!(), // Already checked is_ipv4
            })
            .ok_or_else(|| format!("Interface '{}' has no IPv4 address", interface_name))?;

        let local_ip_address: IpAddress = local_ip.octets();

        // Bind the UDP socket to the local IP and the standard POWERLINK SDO port
        let local_sock_addr = SocketAddr::from((local_ip, C_SDO_EPL_PORT));
        let udp_socket = UdpSocket::bind(local_sock_addr)
            .map_err(|e| format!("Failed to bind UDP socket to {}: {}", local_sock_addr, e))?;

        // Set UDP socket to non-blocking for receive_udp
        udp_socket
            .set_nonblocking(true)
            .map_err(|e| format!("Failed to set UDP socket non-blocking: {}", e))?;

        Ok(Self {
            tx_raw: Mutex::new(tx_raw),
            rx_raw: Mutex::new(rx_raw),
            pnet_iface: interface,
            node_id,
            mac_address,
            udp_socket: Arc::new(udp_socket),
            local_ip_address,
        })
    }

    /// Sets the read timeout for the underlying *raw Ethernet* channel.
    /// Re-creates the raw channel.
    pub fn set_read_timeout(&mut self, duration: Duration) -> Result<(), PowerlinkError> {
        let config = datalink::Config {
            read_timeout: Some(duration),
            promiscuous: true,
            ..Default::default()
        };

        match datalink::channel(&self.pnet_iface, config) {
            Ok(Channel::Ethernet(tx, rx)) => {
                *self.tx_raw.lock().unwrap() = tx;
                *self.rx_raw.lock().unwrap() = rx;
                Ok(())
            }
            Ok(_) => Err(PowerlinkError::IoError),
            Err(e) => {
                eprintln!("Failed to set read timeout for raw socket: {}", e);
                Err(PowerlinkError::IoError)
            }
        }
    }
}

impl NetworkInterface for LinuxPnetInterface {
    // --- Raw Ethernet Methods ---
    fn send_frame(&mut self, frame: &[u8]) -> Result<(), PowerlinkError> {
        self.tx_raw
            .lock()
            .unwrap()
            .send_to(frame, None)
            .ok_or(PowerlinkError::IoError)? // For channel closed
            .map_err(|_| PowerlinkError::IoError)?; // For OS error
        Ok(())
    }

    fn receive_frame(&mut self, buffer: &mut [u8]) -> Result<usize, PowerlinkError> {
        let mut rx_guard = self.rx_raw.lock().unwrap();
        match rx_guard.next() {
            Ok(frame) => {
                let len = frame.len();
                if buffer.len() >= len {
                    buffer[..len].copy_from_slice(frame);
                    Ok(len)
                } else {
                    Err(PowerlinkError::BufferTooShort)
                }
            }
            Err(e) => {
                if e.kind() == io::ErrorKind::TimedOut {
                    Ok(0) // Return 0 bytes on timeout
                } else {
                    Err(PowerlinkError::IoError)
                }
            }
        }
    }

    fn local_node_id(&self) -> u8 {
        self.node_id
    }

    fn local_mac_address(&self) -> [u8; 6] {
        self.mac_address
    }

    // --- UDP Methods (Implementation always compiled for this crate) ---
    // Removed #[cfg(feature = "sdo-udp")] guards from implementation methods
    fn send_udp(
        &mut self,
        dest_ip: IpAddress,
        dest_port: u16,
        data: &[u8],
    ) -> Result<(), PowerlinkError> {
        // Convert destination IP and port to SocketAddr
        let dest_sock_addr = SocketAddr::from((dest_ip, dest_port));
        // Use send_to on the UdpSocket
        self.udp_socket
            .send_to(data, dest_sock_addr)
            .map(|_bytes_sent| ()) // Discard the number of bytes sent on success
            .map_err(|e| {
                eprintln!("UDP send_to error: {}", e); // Log the specific IO error
                PowerlinkError::IoError
            })
    }

    // Removed #[cfg(feature = "sdo-udp")] guards from implementation methods
    fn receive_udp(
        &mut self,
        buffer: &mut [u8],
    ) -> Result<Option<(usize, IpAddress, u16)>, PowerlinkError> {
        match self.udp_socket.recv_from(buffer) {
            Ok((size, src_sock_addr)) => {
                // Successfully received data
                let src_ip = match src_sock_addr.ip() {
                    std::net::IpAddr::V4(ip4) => ip4.octets(),
                    std::net::IpAddr::V6(_) => {
                        // Skip IPv6 packets if received
                        eprintln!("Warning: Received UDP packet from IPv6 address, skipping.");
                        return Ok(None);
                    }
                };
                let src_port = src_sock_addr.port();
                Ok(Some((size, src_ip, src_port)))
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                // No data available on non-blocking socket
                Ok(None)
            }
            Err(e) => {
                // Other I/O error
                eprintln!("UDP recv_from error: {}", e); // Log the specific IO error
                Err(PowerlinkError::IoError)
            }
        }
    }

    // Removed #[cfg(feature = "sdo-udp")] guards from implementation methods
    fn local_ip_address(&self) -> IpAddress {
        self.local_ip_address
    }
}
