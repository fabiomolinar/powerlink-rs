#![cfg(target_os = "windows")]

use pnet::datalink::{self, Channel, NetworkInterface as PnetInterface};
use powerlink_rs::{
    hal::PowerlinkError,
    types::{C_SDO_EPL_PORT, IpAddress},
    NetworkInterface,
};
use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct WindowsPnetInterface {
    // Raw Ethernet handling
    tx: Mutex<Box<dyn datalink::DataLinkSender>>,
    rx: Mutex<Box<dyn datalink::DataLinkReceiver>>,
    pnet_iface: PnetInterface,
    node_id: u8,
    mac_address: [u8; 6],
    // UDP Handling
    udp_socket: Arc<UdpSocket>,
    local_ip_address: IpAddress,
}

impl WindowsPnetInterface {
    pub fn new(interface_name: &str, node_id: u8) -> Result<Self, String> {
        let interface = datalink::interfaces()
            .into_iter()
            .find(|iface: &PnetInterface| iface.name == interface_name)
            .ok_or_else(|| format!("Interface '{}' not found", interface_name))?;

        let mac_address = interface.mac.ok_or("Interface has no MAC address")?.into();

        // --- Raw Ethernet Setup ---
        let config_raw = datalink::Config {
            read_timeout: Some(Duration::from_millis(1)),
            promiscuous: true,
            ..Default::default()
        };
        let (tx, rx) = match datalink::channel(&interface, config_raw) {
            Ok(Channel::Ethernet(tx, rx)) => (tx, rx),
            Ok(_) => return Err("Unsupported raw channel type".to_string()),
            Err(e) => return Err(format!("Raw channel error: {}", e)),
        };

        // --- UDP Socket Setup ---
        let local_ip = interface
            .ips
            .iter()
            .find(|ip_net| ip_net.is_ipv4())
            .map(|ip_net| match ip_net.ip() {
                std::net::IpAddr::V4(ipv4) => ipv4,
                _ => unreachable!(),
            })
            .ok_or_else(|| format!("Interface '{}' has no IPv4 address", interface_name))?;

        let local_ip_address: IpAddress = local_ip.octets();
        let local_sock_addr = SocketAddr::from((local_ip, C_SDO_EPL_PORT));
        let udp_socket = UdpSocket::bind(local_sock_addr)
            .map_err(|e| format!("Failed to bind UDP socket to {}: {}", local_sock_addr, e))?;
        udp_socket
            .set_nonblocking(true)
            .map_err(|e| format!("Failed to set UDP socket non-blocking: {}", e))?;

        Ok(Self {
            tx: Mutex::new(tx),
            rx: Mutex::new(rx),
            pnet_iface: interface,
            node_id,
            mac_address,
            udp_socket: Arc::new(udp_socket),
            local_ip_address,
        })
    }

    /// Sets the read timeout for the underlying *raw Ethernet* channel.
    pub fn set_read_timeout(&mut self, duration: Duration) -> Result<(), PowerlinkError> {
        let config = datalink::Config {
            read_timeout: Some(duration),
            promiscuous: true,
            ..Default::default()
        };

        match datalink::channel(&self.pnet_iface, config) {
            Ok(Channel::Ethernet(tx, rx)) => {
                *self.tx.lock().unwrap() = tx;
                *self.rx.lock().unwrap() = rx;
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

impl NetworkInterface for WindowsPnetInterface {
    fn send_frame(&mut self, frame: &[u8]) -> Result<(), PowerlinkError> {
        self.tx
            .lock()
            .unwrap()
            .send_to(frame, None)
            .ok_or(PowerlinkError::IoError)?
            .map_err(|_| PowerlinkError::IoError)?;
        Ok(())
    }

    fn receive_frame(&mut self, buffer: &mut [u8]) -> Result<usize, PowerlinkError> {
        let mut rx_guard = self.rx.lock().unwrap();
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
                    Ok(0)
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

    // --- UDP Methods ---
    fn send_udp(
        &mut self,
        dest_ip: IpAddress,
        dest_port: u16,
        data: &[u8],
    ) -> Result<(), PowerlinkError> {
        let dest_sock_addr = SocketAddr::from((dest_ip, dest_port));
        self.udp_socket
            .send_to(data, dest_sock_addr)
            .map(|_| ())
            .map_err(|e| {
                eprintln!("UDP send_to error: {}", e);
                PowerlinkError::IoError
            })
    }

    fn receive_udp(
        &mut self,
        buffer: &mut [u8],
    ) -> Result<Option<(usize, IpAddress, u16)>, PowerlinkError> {
        match self.udp_socket.recv_from(buffer) {
            Ok((size, src_sock_addr)) => {
                let src_ip = match src_sock_addr.ip() {
                    std::net::IpAddr::V4(ip4) => ip4.octets(),
                    std::net::IpAddr::V6(_) => {
                        eprintln!("Warning: Received UDP packet from IPv6 address, skipping.");
                        return Ok(None);
                    }
                };
                let src_port = src_sock_addr.port();
                Ok(Some((size, src_ip, src_port)))
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(e) => {
                eprintln!("UDP recv_from error: {}", e);
                Err(PowerlinkError::IoError)
            }
        }
    }

    fn local_ip_address(&self) -> IpAddress {
        self.local_ip_address
    }
}