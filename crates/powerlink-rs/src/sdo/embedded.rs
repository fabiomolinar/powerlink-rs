use crate::PowerlinkError;
use crate::frame::Codec;
use crate::sdo::command::{CommandId, Segmentation};
use alloc::vec::Vec;

/// Represents the 1-byte sequence layer header for SDOs embedded in PDOs.
/// (Reference: EPSG DS 301, Table 87)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PdoSequenceLayerHeader {
    pub sequence_number: u8,  // 0-63
    pub connection_state: u8, // 0-3
}

impl Codec for PdoSequenceLayerHeader {
    fn serialize(&self, buffer: &mut [u8]) -> Result<usize, PowerlinkError> {
        if buffer.is_empty() {
            return Err(PowerlinkError::BufferTooShort);
        }
        buffer[0] = (self.sequence_number << 2) | self.connection_state;
        Ok(1)
    }

    fn deserialize(buffer: &[u8]) -> Result<Self, PowerlinkError> {
        if buffer.is_empty() {
            return Err(PowerlinkError::BufferTooShort);
        }
        Ok(Self {
            sequence_number: (buffer[0] >> 2) & 0b0011_1111,
            connection_state: buffer[0] & 0b11,
        })
    }
}

/// Represents an SDO command embedded within a PDO container.
/// Note the differences from the asynchronous SdoCommand:
/// - Uses a 1-byte sequence header.
/// - Has a `valid_payload_length` instead of `segment_size`.
/// (Reference: EPSG DS 301, Table 85)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdoSdoCommand {
    pub sequence_header: PdoSequenceLayerHeader,
    pub transaction_id: u8,
    pub is_response: bool,
    pub is_aborted: bool,
    pub segmentation: Segmentation,
    pub valid_payload_length: u8,
    pub command_id: CommandId,
    pub index: u16,
    pub sub_index: u8,
    // The rest of the payload.
    pub data: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pdo_sequence_header_roundtrip() {
        let original = PdoSequenceLayerHeader {
            sequence_number: 10,
            connection_state: 2, // Connection valid
        };
        let mut buffer = [0u8; 1];
        original.serialize(&mut buffer).unwrap();

        // Expected: (10 << 2) | 2 = 40 | 2 = 42 = 0x2A
        assert_eq!(buffer[0], 0x2A);

        let deserialized = PdoSequenceLayerHeader::deserialize(&buffer).unwrap();
        assert_eq!(original, deserialized);
    }
}
