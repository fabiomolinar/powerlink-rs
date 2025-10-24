// crates/powerlink-io-linux/src/lib.rs
#![cfg(target_os = "linux")]

use pnet::datalink::{self, Channel, NetworkInterface as PnetInterface};
use powerlink_rs::{NetworkInterface, PowerlinkError};
use std::io;
use std::sync::Mutex;
use std::time::Duration;

pub struct LinuxPnetInterface {
    tx: Mutex<Box<dyn datalink::DataLinkSender>>,
    rx: Mutex<Box<dyn datalink::DataLinkReceiver>>,
    pnet_iface: PnetInterface, // Store the interface for re-configuration
    node_id: u8,
    mac_address: [u8; 6],
}

impl LinuxPnetInterface {
    pub fn new(interface_name: &str, node_id: u8) -> Result<Self, String> {
        let interface = datalink::interfaces()
            .into_iter()
            .find(|iface: &PnetInterface| iface.name == interface_name)
            .ok_or_else(|| format!("Interface '{}' not found", interface_name))?;

        let mac_address = interface.mac.ok_or("Interface has no MAC address")?.into();

        // Configure the channel to be promiscuous and have a default read timeout.
        let config = datalink::Config {
            read_timeout: Some(Duration::from_millis(100)),
            promiscuous: true,
            ..Default::default()
        };

        let (tx, rx) = match datalink::channel(&interface, config) {
            Ok(Channel::Ethernet(tx, rx)) => (tx, rx),
            Ok(_) => return Err("Unsupported channel type".to_string()),
            Err(e) => return Err(e.to_string()),
        };

        Ok(Self {
            tx: Mutex::new(tx),
            rx: Mutex::new(rx),
            pnet_iface: interface, // Store the interface
            node_id,
            mac_address,
        })
    }

    /// Sets the read timeout for the underlying network channel.
    /// This re-creates the channel, as pnet config is set at creation time.
    pub fn set_read_timeout(&mut self, duration: Duration) -> Result<(), PowerlinkError> {
        let config = datalink::Config {
            read_timeout: Some(duration),
            promiscuous: true,
            ..Default::default()
        };

        match datalink::channel(&self.pnet_iface, config) {
            Ok(Channel::Ethernet(tx, rx)) => {
                // Replace the old sender and receiver with the new ones
                *self.tx.lock().unwrap() = tx;
                *self.rx.lock().unwrap() = rx;
                Ok(())
            }
            Ok(_) => Err(PowerlinkError::IoError),
            Err(e) => {
                println!("Failed to set read timeout: {}", e);
                Err(PowerlinkError::IoError)
            }
        }
    }
}

impl NetworkInterface for LinuxPnetInterface {
    fn send_frame(&mut self, frame: &[u8]) -> Result<(), PowerlinkError> {
        self.tx
            .lock()
            .unwrap()
            .send_to(frame, None)
            .ok_or(PowerlinkError::IoError)? // For channel closed
            .map_err(|_| PowerlinkError::IoError)?; // For OS error
        Ok(())
    }

    fn receive_frame(&mut self, buffer: &mut [u8]) -> Result<usize, PowerlinkError> {
        // 1. Acquire the lock and bind it.
        let mut rx_guard = self.rx.lock().unwrap();

        // 2. Call next() on the guard.
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
                    Ok(0) // Return 0 bytes on timeout, not an error
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
}
