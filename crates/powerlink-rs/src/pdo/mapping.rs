// In crates/powerlink-rs/src/pdo/mapping.rs
use crate::types::{UNSIGNED8, UNSIGNED16, UNSIGNED64};
use core::fmt;

/// Represents the 8-bit PDO Version, used for mapping validation.
/// (EPSG DS 301, Section 6.4.2)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PDOVersion(pub u8);

/// Represents a validated payload size.
pub struct PayloadSize(pub u16);

/// Error type for invalid payload size creation.
#[derive(Debug, PartialEq, Eq)]
pub enum PayloadSizeError {
    /// payload size is outside the valid range.
    InvalidRange(u16),
}

impl fmt::Display for PayloadSizeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PayloadSizeError::InvalidRange(value) => {
                write!(f, "Invalid payload size {}. Valid range is 0-1490", value)
            }
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for PayloadSizeError {}

impl TryFrom<u16> for PayloadSize {
    type Error = PayloadSizeError;
    /// Valid payload sizes 0 to C_DLL_ISOCHR_MAX_PAYL (1490)
    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            0..=crate::types::C_DLL_ISOCHR_MAX_PAYL => Ok(PayloadSize(value)),
            _ => Err(PayloadSizeError::InvalidRange(value)),
        }
    }
}

impl From<PayloadSize> for u16 {
    /// Converts a `PayloadSize` back into its underlying `u16` representation.
    fn from(payload_size: PayloadSize) -> Self {
        payload_size.0
    }
}

/// Represents a single PDO mapping entry, parsed from a 64-bit value.
/// (EPSG DS 301, Table 93)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PdoMappingEntry {
    /// Object Dictionary index of the object to be mapped.
    pub index: UNSIGNED16,
    /// Object Dictionary sub-index of the object to be mapped.
    pub sub_index: UNSIGNED8,
    /// Offset of the object within the PDO frame, in bits.
    pub offset_bits: UNSIGNED16,
    /// Length of the object in bits.
    pub length_bits: UNSIGNED16,
}

impl PdoMappingEntry {
    /// Deserializes a UNSIGNED64 value from the OD into a mapping entry.
    pub fn from_u64(value: UNSIGNED64) -> Self {
        Self {
            index: (value & 0xFFFF) as UNSIGNED16,
            sub_index: ((value >> 16) & 0xFF) as UNSIGNED8,
            // 8 bits reserved (bits 24-31)
            offset_bits: ((value >> 32) & 0xFFFF) as UNSIGNED16,
            length_bits: ((value >> 48) & 0xFFFF) as UNSIGNED16,
        }
    }

    /// Serializes the mapping entry into a UNSIGNED64 value for storing in the OD.
    pub fn to_u64(&self) -> UNSIGNED64 {
        (self.index as UNSIGNED64)
            | ((self.sub_index as UNSIGNED64) << 16)
            | ((self.offset_bits as UNSIGNED64) << 32)
            | ((self.length_bits as UNSIGNED64) << 48)
    }

    /// Helper to get the offset in bytes, assuming byte alignment.
    /// Returns None if not byte-aligned.
    pub fn byte_offset(&self) -> Option<usize> {
        if self.offset_bits % 8 == 0 {
            Some(self.offset_bits as usize / 8)
        } else {
            None // Bit-level mapping not supported yet
        }
    }

    /// Helper to get the length in bytes, assuming byte alignment.
    /// Returns None if not byte-aligned.
    pub fn byte_length(&self) -> Option<usize> {
        if self.length_bits % 8 == 0 {
            Some(self.length_bits as usize / 8)
        } else {
            None // Bit-level mapping not supported yet
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pdo_mapping_entry_roundtrip() {
        let entry = PdoMappingEntry {
            index: 0x6000,
            sub_index: 0x01,
            offset_bits: 32, // 4 bytes
            length_bits: 16, // 2 bytes
        };

        let raw_u64 = entry.to_u64();
        // Length [16] @ 48 | Offset [32] @ 32 | SubIndex [1] @ 16 | Index [0x6000] @ 0
        let expected_u64 = 0x0010_0020_00_01_6000;
        assert_eq!(raw_u64, expected_u64);

        let parsed_entry = PdoMappingEntry::from_u64(raw_u64);
        assert_eq!(entry, parsed_entry);
    }
}
