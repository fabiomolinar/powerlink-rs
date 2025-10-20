#![cfg(target_os = "windows")]

use powerlink_rs::{NetworkInterface, PowerlinkError};
use pnet::datalink::{self, Channel, NetworkInterface as PnetInterface};
use std::sync::Mutex;

pub struct WindowsPnetInterface {
    tx: Mutex<Box<dyn datalink::DataLinkSender>>,
    rx: Mutex<Box<dyn datalink::DataLinkReceiver>>,
    node_id: u8,
    mac_address: [u8; 6],
}

impl WindowsPnetInterface {
    pub fn new(interface_name: &str, node_id: u8) -> Result<Self, String> {
        let interface = datalink::interfaces()
            .into_iter()
            .find(|iface: &PnetInterface| iface.name == interface_name)
            .ok_or_else(|| format!("Interface '{}' not found", interface_name))?;

        let mac_address = interface.mac.ok_or("Interface has no MAC address")?.into();

        let (tx, rx) = match datalink::channel(&interface, Default::default()) {
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
        // 1. Acquire the lock and bind it to a variable to extend its lifetime.
        let mut rx_guard = self.rx.lock().unwrap();
        
        // 2. Call next() on the guard.
        let frame = rx_guard.next().map_err(|_| PowerlinkError::IoError)?;
        
        // The lock is now held until the end of this function.
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