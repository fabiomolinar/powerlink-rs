use crate::types::InvalidMessageTypeError;
use core::array::TryFromSliceError;
use core::fmt;

/// Defines a portable, descriptive Error type for the POWERLINK stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerlinkError {
    /// The provided buffer is too small for the operation.
    BufferTooShort,
    /// An underlying I/O error occurred.
    IoError,
    /// A received frame is fundamentally invalid (e.g., wrong EtherType, bad message type).
    InvalidFrame,
    /// A value in a frame is not a valid enum variant.
    InvalidEnumValue,
    /// A multi-byte value could not be parsed from a slice.
    SliceConversion,
    /// The frame size exceeds the maximum physical or configured MTU.
    FrameTooLarge,
    /// The device is not yet configured or ready to transmit/receive.
    NotReady,
    /// The requested Object Dictionary index does not exist.
    ObjectNotFound,
    /// The requested sub-index does not exist for the given object.
    SubObjectNotFound,
    /// An attempt was made to write a value with an incorrect data type to an object.
    TypeMismatch,
    /// Invalid Node ID value encountered.
    InvalidNodeId,
}

// --- From Implementations for Error Conversion ---

impl From<InvalidMessageTypeError> for PowerlinkError {
    fn from(_: InvalidMessageTypeError) -> Self {
        PowerlinkError::InvalidEnumValue
    }
}

impl From<TryFromSliceError> for PowerlinkError {
    fn from(_: TryFromSliceError) -> Self {
        PowerlinkError::SliceConversion
    }
}

// --- Display trait for better error messages ---
impl fmt::Display for PowerlinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BufferTooShort => write!(f, "Buffer is too short for the frame"),
            Self::IoError => write!(f, "An underlying I/O error occurred"),
            Self::InvalidFrame => write!(f, "Frame is invalid or malformed"),
            Self::InvalidEnumValue => write!(f, "A value does not correspond to a valid enum variant"),
            Self::SliceConversion => write!(f, "Failed to convert slice to a fixed-size array"),
            Self::FrameTooLarge => write!(f, "Frame size exceeds maximum allowed MTU"),
            Self::NotReady => write!(f, "Device is not ready or configured"),
            Self::ObjectNotFound => write!(f, "The requested Object Dictionary index was not found"),
            Self::SubObjectNotFound => write!(f, "The requested sub-index was not found for this object"),
            Self::TypeMismatch => write!(f, "The provided value's type does not match the object's type"),                        
            Self::InvalidNodeId => write!(f, "The Node ID value is invalid"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for PowerlinkError {}

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