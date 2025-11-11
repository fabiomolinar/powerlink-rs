// crates/powerlink-rs/src/hal.rs
use crate::od::ObjectValue;
use crate::pdo::PayloadSizeError;
use crate::pdo::PdoError; // Added import for PdoError
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
    /// A received frame is fundamentally invalid (e.g., wrong EtherType).
    InvalidEthernetFrame,
    /// A received powerlink frame is fundamentally invalid (e.g., too short for headers).
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
    /// A value in a frame is not a valid enum variant (e.g., Segmentation, CommandId).
    InvalidEnumValue,
    /// A multi-byte value could not be parsed from a slice (often due to wrong length).
    SliceConversion,
    /// The frame size exceeds the maximum physical or configured MTU.
    FrameTooLarge,
    /// The device is not yet configured or ready to transmit/receive.
    NotReady,
    /// The requested Object Dictionary index does not exist.
    ObjectNotFound,
    /// The requested sub-index does not exist for the given object.
    SubObjectNotFound,
    /// An attempt was made to write/deserialize a value with an incorrect data type.
    TypeMismatch,
    /// An error occurred in the storage backend.
    StorageError(&'static str),
    /// A mandatory object was missing or invalid during validation.
    ValidationError(&'static str),
    /// SDO Sequence number was unexpected or connection state mismatch.
    SdoSequenceError(&'static str),
    /// SDO command layer received an abort message.
    SdoAborted(u32), // Include abort code
    /// SDO command payload could not be parsed correctly (e.g., ReadByIndexRequest format).
    SdoInvalidCommandPayload,
    /// A configured PDO mapping exceeds the available payload size for that channel.
    PdoMapOverrun,
    /// Internal logic error.
    InternalError(&'static str),
}

impl fmt::Display for PowerlinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BufferTooShort => write!(f, "Buffer is too short"),
            Self::IoError => write!(f, "Underlying I/O error"),
            Self::InvalidEthernetFrame => {
                write!(f, "Invalid Ethernet frame (e.g., wrong EtherType)")
            }
            Self::InvalidPlFrame => write!(f, "Invalid POWERLINK frame (e.g., too short)"),
            Self::InvalidMessageType(v) => write!(f, "Invalid MessageType: {:#04x}", v),
            Self::InvalidNmtState(v) => write!(f, "Invalid NMT State: {:#04x}", v),
            Self::InvalidServiceId(v) => write!(f, "Invalid ServiceId: {:#04x}", v),
            Self::InvalidRequestedServiceId(v) => {
                write!(f, "Invalid RequestedServiceId: {:#04x}", v)
            }
            Self::InvalidNodeId(v) => write!(f, "Invalid NodeId: {}", v),
            Self::InvalidPayloadSize(v) => write!(f, "Invalid PayloadSize: {}", v),
            Self::InvalidEnumValue => write!(f, "Invalid enum value in frame"),
            Self::SliceConversion => write!(f, "Failed to convert slice to fixed-size array"),
            Self::FrameTooLarge => write!(f, "Frame size exceeds MTU"),
            Self::NotReady => write!(f, "Device not ready or configured"),
            Self::ObjectNotFound => write!(f, "OD index not found"),
            Self::SubObjectNotFound => write!(f, "OD sub-index not found"),
            Self::TypeMismatch => write!(f, "Data type mismatch"),
            Self::StorageError(s) => write!(f, "Storage error: {}", s),
            Self::ValidationError(s) => write!(f, "Validation error: {}", s),
            Self::SdoSequenceError(s) => write!(f, "SDO sequence error: {}", s),
            Self::SdoAborted(code) => write!(f, "SDO transfer aborted with code {:#010X}", code),
            Self::SdoInvalidCommandPayload => write!(f, "Invalid SDO command payload format"),
            Self::PdoMapOverrun => write!(f, "PDO mapping exceeds configured payload size limit"),
            Self::InternalError(s) => write!(f, "Internal error: {}", s),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for PowerlinkError {}

// --- From Implementations for Error Conversion ---

impl From<InvalidMessageTypeError> for PowerlinkError {
    fn from(_: InvalidMessageTypeError) -> Self {
        PowerlinkError::InvalidEnumValue // Or InvalidMessageType if needed
    }
}

impl From<TryFromSliceError> for PowerlinkError {
    fn from(_: TryFromSliceError) -> Self {
        // This often happens due to incorrect slice length passed to try_into
        // Map it to SliceConversion or potentially BufferTooShort depending on context
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

/// Converts a PdoError into a PowerlinkError.
impl From<PdoError> for PowerlinkError {
    fn from(err: PdoError) -> Self {
        match err {
            PdoError::ObjectNotFound { .. } => PowerlinkError::ObjectNotFound,
            PdoError::TypeMismatch { .. } => PowerlinkError::TypeMismatch,
            PdoError::PayloadTooSmall { .. } => PowerlinkError::BufferTooShort,
            // Convert the String-based error to a static string.
            // We lose the specific message, but gain the correct error type.
            PdoError::ConfigurationError(_) => {
                PowerlinkError::ValidationError("PDO Configuration Error")
            }
        }
    }
}

impl From<&'static str> for PowerlinkError {
    fn from(s: &'static str) -> Self {
        // Generic conversion from string slice, useful for internal errors
        PowerlinkError::InternalError(s)
    }
}

/// Hardware Abstraction Layer (HAL) for network communication.
///
/// This trait abstracts the physical sending and receiving of raw Ethernet frames
/// and UDP datagrams, enabling the core POWERLINK protocol logic to remain
/// platform-agnostic (no_std).
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
    /// Returns Ok(0) specifically on a read timeout if configured.
    /// The buffer must be large enough to hold the maximum possible Ethernet frame (e'g', 1518 bytes).
    fn receive_frame(&mut self, buffer: &mut [u8]) -> Result<usize, PowerlinkError>;

    /// Returns the Node ID assigned to this local device (Managing Node or Controlled Node).
    fn local_node_id(&self) -> u8;

    /// Returns the local MAC address of the interface.
    fn local_mac_address(&self) -> [u8; 6];

    /// Sends a UDP datagram. Only available when the `sdo-udp` feature is enabled.
    ///
    /// `dest_ip`: The destination IPv4 address.
    /// `dest_port`: The destination UDP port.
    /// `data`: The payload to send.
    #[cfg(feature = "sdo-udp")]
    fn send_udp(
        &mut self,
        dest_ip: crate::types::IpAddress,
        dest_port: u16,
        data: &[u8],
    ) -> Result<(), PowerlinkError>;

    /// Attempts to receive a single UDP datagram. Only available when the `sdo-udp` feature is enabled.
    ///
    /// This function may block or timeout depending on the HAL implementation.
    ///
    /// Returns `Ok(Some((size, source_ip, source_port)))` on success.
    /// Returns `Ok(None)` if no datagram is received within a configured timeout (if supported).
    /// Returns `Err(...)` on error.
    /// The buffer must be large enough for the expected UDP payload + headers if applicable by the HAL.
    #[cfg(feature = "sdo-udp")]
    fn receive_udp(
        &mut self,
        buffer: &mut [u8],
    ) -> Result<Option<(usize, crate::types::IpAddress, u16)>, PowerlinkError>;

    /// Returns the local IP address of the interface. Only available when the `sdo-udp` feature is enabled.
    ///
    /// Returns a default or unspecifed address (e.g., 0.0.0.0) if the IP address is not configured or available.
    #[cfg(feature = "sdo-udp")]
    fn local_ip_address(&self) -> crate::types::IpAddress;
}

/// A trait for abstracting the non-volatile storage of OD parameters.
/// This abstraction is crucial for the "Restore Defaults" functionality,
/// which must persist across device reboots.
pub trait ObjectDictionaryStorage {
    /// Loads storable parameters from non-volatile memory.
    /// Returns a map of (Index, SubIndex) -> Value.
    fn load(&mut self) -> Result<BTreeMap<(u16, u8), ObjectValue>, PowerlinkError>;

    /// Saves the given storable parameters to non-volatile memory.
    fn save(&mut self, parameters: &BTreeMap<(u16, u8), ObjectValue>)
    -> Result<(), PowerlinkError>;

    /// Clears all stored parameters from non-volatile memory.
    fn clear(&mut self) -> Result<(), PowerlinkError>;

    /// Checks if a "Restore Defaults" operation has been requested and is pending a reboot.
    /// This should check for a persistent flag set by `request_restore_defaults`.
    fn restore_defaults_requested(&self) -> bool;

    /// Sets a persistent flag to indicate that defaults should be restored on the next boot.
    /// This is called when the "load" signature is written to OD entry 0x1011.
    fn request_restore_defaults(&mut self) -> Result<(), PowerlinkError>;

    /// Clears the persistent "Restore Defaults" flag. This should be called
    /// after the restore operation has been completed on boot.
    fn clear_restore_defaults_flag(&mut self) -> Result<(), PowerlinkError>;
}
