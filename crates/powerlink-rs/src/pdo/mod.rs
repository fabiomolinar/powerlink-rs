use crate::types::{C_DLL_ISOCHR_MAX_PAYL};
use core::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PDOVersion(pub u8); // Placeholder for future use

pub struct PayloadSize(pub u16); 

/// Error type for invalid payload size creation.
#[derive(Debug, PartialEq, Eq)]
pub enum PayloadSizeError {
    /// payload size is outside the valid range (1-240, 254, 255).
    InvalidRange(u16),
}

impl fmt::Display for PayloadSizeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PayloadSizeError::InvalidRange(value) => write!(f, "Invalid payload size {}. Valid range is 0-{}", value, C_DLL_ISOCHR_MAX_PAYL),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for PayloadSizeError {}

impl TryFrom<u16> for PayloadSize {
    type Error = PayloadSizeError;

    /// Valid payload sizes 0 to C_DLL_ISOCHR_MAX_PAYL
    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            // Regular CN and MN range
            0..=C_DLL_ISOCHR_MAX_PAYL => Ok(PayloadSize(value)),
            // All other values are invalid
            _ => Err(PayloadSizeError::InvalidRange(value)),
        }
    }
}

impl From<PayloadSize> for u16 {
    /// Converts a `PayloadSize` back into its underlying `u16` representation.
    /// This conversion is infallible.
    fn from(payload_size: PayloadSize) -> Self {
        payload_size.0
    }
}