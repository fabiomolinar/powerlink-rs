/// Define a portable Error type compatible with both no_std and std environments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "std", derive(thiserror::Error))]
#[cfg_attr(feature = "std", error("POWERLINK HAL Error: {0}"))]
pub enum PowerlinkError {
    /// The frame size exceeds the maximum physical or configured MTU.
    FrameTooLarge,
    /// An underlying I/O error occurred (requires standard library to elaborate on source).
    IoError,
    /// The received frame failed a basic validity check (e.g., CRC or fundamental format error).
    InvalidFrame,
    /// The device is not yet configured or ready to transmit/receive.
    NotReady,
    // Add more low-level protocol errors as needed (e.g., buffer overflow detection).
}

/// Hardware Abstraction Layer (HAL) for raw Ethernet packet transmission.
///
/// This trait abstracts the physical sending and receiving of raw Ethernet frames,
/// enabling the core POWERLINK protocol logic to remain platform-agnostic (no_std).
pub trait NetworkInterface {
    /// Sends a raw Ethernet frame (including Ethernet header) over the network.
    ///
    /// `frame`: The byte slice containing the complete Ethernet frame.
    fn send_frame(&mut self, frame: &[u8]) -> Result<(), PowerlinkError>;

    /// Attempts to receive a single raw Ethernet frame into the provided buffer.
    /// This function should block until a frame is received or return an error/None
    /// if non-blocking operation is configured.
    ///
    /// Returns the number of bytes read if successful, or an error.
    /// The buffer must be large enough to hold the maximum possible Ethernet frame (e'g', 1518 bytes).
    fn receive_frame(&mut self, buffer: &mut [u8]) -> Result<usize, PowerlinkError>;

    /// Returns the Node ID assigned to this local device (Managing Node or Controlled Node).
    fn local_node_id(&self) -> u8;

    /// Returns the local MAC address of the interface.
    fn local_mac_address(&self) -> [u8; 6];
}