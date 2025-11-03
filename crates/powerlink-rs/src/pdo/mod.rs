//! Process Data Object (PDO) logic.
//!
//! This module handles the mapping, packing, and unpacking of
//! real-time data (PDOs) exchanged during the isochronous phase.

// 'alloc' is used for error messages
extern crate alloc;
use alloc::format;
use alloc::string::String;

pub mod mapping;
pub use mapping::PdoMappingEntry;

/// Represents the PDO Communication Parameters stored in objects
/// 14xxh (RPDO) and 18xxh (TPDO). 
///
/// This struct holds the data from `PDO_CommParamRecord_TYPE` (0420h). [cite: 4758]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PdoCommParams {
    /// For RPDO: The Node ID of the producer transmitting this PDO. [cite: 4751]
    ///           0 indicates the PDO is from a PReq.
    /// For TPDO: The Node ID of the PReq target (for MN) or 0 (for CN). [cite: 4754]
    pub node_id: u8,
    /// The version of this PDO mapping, used for configuration checks. 
    pub mapping_version: u8,
}

/// Errors that can occur during PDO processing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PdoError {
    /// A mapping entry points to an object that does not exist or is not mappable.
    ObjectNotFound { index: u16, sub_index: u8 },
    /// The mapped object's type in the OD does not match the size
    /// specified in the PDO mapping (e.g., mapping 16 bits to a U32).
    TypeMismatch {
        index: u16,
        sub_index: u8,
        expected_bits: u16,
        actual_bits: u16,
    },
    /// The PDO payload is too small to contain the data described by the mapping.
    PayloadTooSmall {
        expected_bits: u16,
        actual_bytes: usize,
    },
    /// A configuration error, e.g., mapping exceeds buffer limits.
    ConfigurationError(String),
}

#[cfg(feature = "std")]
impl std::error::Error for PdoError {}

impl core::fmt::Display for PdoError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ObjectNotFound { index, sub_index } => {
                write!(
                    f,
                    "PDO mapping error: object {:#06X}/{:#04X} not found in OD",
                    index, sub_index
                )
            }
            Self::TypeMismatch {
                index,
                sub_index,
                expected_bits,
                actual_bits,
            } => {
                write!(
                    f,
                    "PDO mapping error for {:#06X}/{:#04X}: mapping specifies {} bits, but OD object has {} bits",
                    index, sub_index, expected_bits, actual_bits
                )
            }
            Self::PayloadTooSmall {
                expected_bits,
                actual_bytes,
            } => {
                write!(
                    f,
                    "RPDO payload is too small: mapping requires {} bits, but payload is only {} bytes",
                    expected_bits, actual_bytes
                )
            }
            Self::ConfigurationError(s) => write!(f, "PDO configuration error: {}", s),
        }
    }
}