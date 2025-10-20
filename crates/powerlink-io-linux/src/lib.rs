#![cfg(target_os = "linux")]

use powerlink_rs::{NetworkInterface, PowerlinkError};
use pnet::datalink::{self, Channel, NetworkInterface as PnetInterface};
use std::sync::Mutex;
use std::time::Duration;

pub struct LinuxPnetInterface {
    tx: Mutex<Box<dyn datalink::DataLinkSender>>,
    // Use a receiver that can be configured with a timeout
    rx: Mutex<Box<dyn datalink::DataLinkReceiver>>,
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

        // Configure the channel to be promiscuous and have a read timeout.
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
            node_id,
            mac_address,
        })
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
        let frame = match rx_guard.next() {
            Ok(frame) => frame,
            Err(e) => {
                if e.kind() == std::io::ErrorKind::TimedOut {
                    return Ok(0);
                }
                return Err(PowerlinkError::IoError);
            }
        };
        
        let len = frame.len();
        if buffer.len() >= len {
            buffer[..len].copy_from_slice(frame);
            Ok(len)
        } else {
            Err(PowerlinkError::BufferTooShort)
        }
    }

    fn local_node_id(&self) -> u8 {
        self.node_id
    }

    fn local_mac_address(&self) -> [u8; 6] {
        self.mac_address
    }
}