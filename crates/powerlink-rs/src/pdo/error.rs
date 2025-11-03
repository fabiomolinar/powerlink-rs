// 'alloc' is used for error messages
extern crate alloc;
use alloc::string::String;

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
    /// The PDO payload or buffer is too small to contain the data described by the mapping.
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
                    "PDO payload/buffer is too small: mapping requires {} bits, but size is only {} bytes",
                    expected_bits, actual_bytes
                )
            }
            Self::ConfigurationError(s) => write!(f, "PDO configuration error: {}", s),
        }
    }
}
