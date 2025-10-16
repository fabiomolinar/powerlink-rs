use crate::od::ObjectValue;
use crate::pdo::PayloadSizeError;
use crate::types::{InvalidMessageTypeError, NodeIdError};
use alloc::collections::BTreeMap;
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
    InvalidEthernetFrame,
    /// A received powerlink frame is fundamentally invalid (e.g., too short to contain required headers).
    InvalidPlFrame,
    /// A value in the frame is not a valid MessageType.
    InvalidMessageType(u8),
    /// A value in the frame is not a valid NMT State.
    InvalidNmtState(u8),
    /// A value in the frame is not a valid ServiceId.
    InvalidServiceId(u8),
    /// A value in the frame is not a valid RequestedServiceId.
    InvalidRequestedServiceId(u8),
    /// A value in the frame is not a valid NodeId.
    InvalidNodeId(u8),
    /// A value in the frame is not a valid PayloadSize.
    InvalidPayloadSize(u16),
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
    /// An error occurred in the storage backend.
    StorageError(&'static str),
    /// A mandatory object was missing from the Object Dictionary during validation.
    ValidationError(&'static str),
}

impl fmt::Display for PowerlinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BufferTooShort => write!(f, "Buffer is too short for the frame"),
            Self::IoError => write!(f, "An underlying I/O error occurred"),
            Self::InvalidEthernetFrame => write!(f, "Frame is not a valid Ethernet II frame"),
            Self::InvalidPlFrame => write!(f, "Frame is not a valid POWERLINK frame"),
            Self::InvalidMessageType(v) => write!(f, "Invalid MessageType value: {v:#04x}"),
            Self::InvalidNmtState(v) => write!(f, "Invalid NMT State value: {v:#04x}"),
            Self::InvalidServiceId(v) => write!(f, "Invalid ServiceId value: {v:#04x}"),
            Self::InvalidRequestedServiceId(v) => write!(f, "Invalid RequestedServiceId value: {v:#04x}"),
            Self::InvalidNodeId(v) => write!(f, "Invalid NodeId value: {v}"),
            Self::InvalidPayloadSize(v) => write!(f, "Invalid PayloadSize value: {v}"),
            Self::SliceConversion => write!(f, "Failed to convert slice to a fixed-size array"),
            Self::FrameTooLarge => write!(f, "Frame size exceeds maximum allowed MTU"),
            Self::NotReady => write!(f, "Device is not ready or configured"),
            Self::ObjectNotFound => write!(f, "The requested Object Dictionary index was not found"),
            Self::SubObjectNotFound => write!(f, "The requested sub-index was not found for this object"),
            Self::TypeMismatch => write!(f, "The provided value's type does not match the object's type"), 
            Self::InvalidEnumValue => write!(f, "A value in the frame is not a valid enum variant"),
            Self::StorageError(s) => write!(f, "Storage error: {}", s),
            Self::ValidationError(s) => write!(f, "OD Validation Error: {}", s),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for PowerlinkError {}

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

impl From<NodeIdError> for PowerlinkError {
    fn from(err: NodeIdError) -> Self {
        match err {
            NodeIdError::InvalidRange(val) => PowerlinkError::InvalidNodeId(val),
        }
    }
}

impl From<PayloadSizeError> for PowerlinkError {
    fn from(err: PayloadSizeError) -> Self {
        match err {
            PayloadSizeError::InvalidRange(val) => PowerlinkError::InvalidPayloadSize(val),
        }
    }
}

impl From<&'static str> for PowerlinkError {
    fn from(s: &'static str) -> Self {
        PowerlinkError::StorageError(s)
    }
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

/// A trait for abstracting the non-volatile storage of OD parameters.
/// This abstraction is crucial for the "Restore Defaults" functionality,
/// which must persist across device reboots.
pub trait ObjectDictionaryStorage {
    /// Loads storable parameters from non-volatile memory.
    /// Returns a map of (Index, SubIndex) -> Value.
    fn load(&mut self) -> Result<BTreeMap<(u16, u8), ObjectValue>, &'static str>;

    /// Saves the given storable parameters to non-volatile memory.
    fn save(&mut self, parameters: &BTreeMap<(u16, u8), ObjectValue>) -> Result<(), &'static str>;
    
    /// Clears all stored parameters from non-volatile memory.
    fn clear(&mut self) -> Result<(), &'static str>;

    /// Checks if a "Restore Defaults" operation has been requested and is pending a reboot.
    /// This should check for a persistent flag set by `request_restore_defaults`.
    fn is_restore_requested(&mut self) -> Result<bool, &'static str>;

    /// Sets a persistent flag to indicate that defaults should be restored on the next boot.
    /// This is called when the "load" signature is written to OD entry 0x1011.
    fn request_restore_defaults(&mut self) -> Result<(), &'static str>;

    /// Clears the persistent "Restore Defaults" flag. This should be called
    /// after the restore operation has been completed on boot.
    fn clear_restore_request(&mut self) -> Result<(), &'static str>;
}
