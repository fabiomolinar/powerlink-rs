// 'alloc' is used for error messages
extern crate alloc;
use alloc::format;
use alloc::string::String;
use crate::od::SdoAbortCode;

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
    /// The PDO payload buffer is too small to hold the data described by the mapping.
    PayloadBufferTooSmall {
        expected_bits: u16,
        actual_bytes: usize,
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
            Self::PayloadBufferTooSmall {
                expected_bits,
                actual_bytes,
            } => {
                write!(
                    f,
                    "TPDO buffer is too small: mapping requires {} bits, but buffer is only {} bytes",
                    expected_bits, actual_bytes
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

// Helper to convert OdError to PdoError
impl From<SdoAbortCode> for PdoError {
    fn from(error: SdoAbortCode) -> Self {
        match error {
            // We lose context here, but it's the best we can do.
            SdoAbortCode::ObjectDoesNotExist => PdoError::ObjectNotFound {
                index: 0,
                sub_index: 0,
            },
            _ => PdoError::ConfigurationError(format!("OD access failed: {:?}", error)),
        }
    }
}