// crates/powerlink-rs/tests/simulator/interface.rs
use powerlink_rs::hal::{NetworkInterface, PowerlinkError};
#[cfg(feature = "sdo-udp")]
use powerlink_rs::types::IpAddress;
use std::collections::VecDeque;

/// A simulated network interface that buffers frames in memory.
pub struct SimulatedInterface {
    local_node_id: u8,
    local_mac: [u8; 6],
    /// Incoming frames (from Network to Node)
    rx_queue: VecDeque<Vec<u8>>,
    /// Outgoing frames (from Node to Network)
    tx_queue: VecDeque<Vec<u8>>,
}

impl SimulatedInterface {
    pub fn new(local_node_id: u8, local_mac: [u8; 6]) -> Self {
        Self {
            local_node_id,
            local_mac,
            rx_queue: VecDeque::new(),
            tx_queue: VecDeque::new(),
        }
    }

    /// Pushes a frame into the receive buffer (simulating arrival from wire).
    pub fn push_rx(&mut self, frame: Vec<u8>) {
        self.rx_queue.push_back(frame);
    }

    /// Extracts all pending transmitted frames.
    pub fn take_tx_frames(&mut self) -> Vec<Vec<u8>> {
        self.tx_queue.drain(..).collect()
    }
}

impl NetworkInterface for SimulatedInterface {
    fn send_frame(&mut self, frame: &[u8]) -> Result<(), PowerlinkError> {
        self.tx_queue.push_back(frame.to_vec());
        Ok(())
    }

    fn receive_frame(&mut self, buffer: &mut [u8]) -> Result<usize, PowerlinkError> {
        if let Some(frame) = self.rx_queue.pop_front() {
            if buffer.len() < frame.len() {
                return Err(PowerlinkError::BufferTooShort);
            }
            buffer[..frame.len()].copy_from_slice(&frame);
            Ok(frame.len())
        } else {
            // No frame available (non-blocking simulation)
            Ok(0)
        }
    }

    fn local_node_id(&self) -> u8 {
        self.local_node_id
    }

    fn local_mac_address(&self) -> [u8; 6] {
        self.local_mac
    }

    #[cfg(feature = "sdo-udp")]
    fn send_udp(
        &mut self,
        _dest_ip: IpAddress,
        _dest_port: u16,
        _data: &[u8],
    ) -> Result<(), PowerlinkError> {
        Ok(())
    }

    #[cfg(feature = "sdo-udp")]
    fn receive_udp(
        &mut self,
        _buffer: &mut [u8],
    ) -> Result<Option<(usize, IpAddress, u16)>, PowerlinkError> {
        Ok(None)
    }

    #[cfg(feature = "sdo-udp")]
    fn local_ip_address(&self) -> IpAddress {
        [192, 168, 100, self.local_node_id]
    }
}