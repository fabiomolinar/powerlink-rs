use crate::PowerlinkError;
use crate::sdo::command::{CommandId, Segmentation};
use alloc::vec::Vec;

/// Represents the 1-byte sequence layer header for SDOs embedded in PDOs.
/// (Reference: EPSG DS 301, Table 87)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PdoSequenceLayerHeader {
    pub sequence_number: u8,  // 0-63
    pub connection_state: u8, // 0-3
}

// This is a payload codec, not a frame codec.
impl PdoSequenceLayerHeader {
    pub fn serialize(&self, buffer: &mut [u8]) -> Result<usize, PowerlinkError> {
        if buffer.is_empty() {
            return Err(PowerlinkError::BufferTooShort);
        }
        buffer[0] = (self.sequence_number << 2) | self.connection_state;
        Ok(1)
    }

    pub fn deserialize(buffer: &[u8]) -> Result<Self, PowerlinkError> {
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
/// Reference: EPSG DS 301, Table 85
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

impl PdoSdoCommand {
    /// Deserializes an embedded SDO command from a PDO container payload.
    /// Assumes the buffer starts *at the sequence layer header*.
    pub fn deserialize(buffer: &[u8]) -> Result<Self, PowerlinkError> {
        if buffer.len() < 8 {
            // Min size: Seq(1) + TID(1) + Flags(1) + VPL(1) + CmdID(1) + Index(2) + SubIndex(1)
            return Err(PowerlinkError::BufferTooShort);
        }

        let sequence_header = PdoSequenceLayerHeader::deserialize(&buffer[0..1])?;
        let transaction_id = buffer[1];
        let flags = buffer[2];
        let valid_payload_length = buffer[3];
        let command_id = CommandId::try_from(buffer[4])?;
        let index = u16::from_le_bytes(buffer[5..7].try_into()?);
        let sub_index = buffer[7];

        let is_response = (flags & 0x10) != 0;
        let is_aborted = (flags & 0x20) != 0;
        let segmentation = Segmentation::try_from((flags >> 6) & 0b11)?;

        // Data size is not present in embedded SDOs (only in async Initiate)
        let data_offset = 8;
        let data_len = valid_payload_length as usize;

        if buffer.len() < data_offset + data_len {
            return Err(PowerlinkError::BufferTooShort);
        }

        let data = buffer[data_offset..data_offset + data_len].to_vec();

        Ok(Self {
            sequence_header,
            transaction_id,
            is_response,
            is_aborted,
            segmentation,
            valid_payload_length,
            command_id,
            index,
            sub_index,
            data,
        })
    }

    /// Serializes the embedded SDO command into a byte vector.
    pub fn serialize(&self) -> Vec<u8> {
        let mut buffer = Vec::with_capacity(8 + self.data.len());

        // 1. Sequence Layer (1 byte)
        buffer.push((self.sequence_header.sequence_number << 2) | self.sequence_header.connection_state);

        // 2. Command Layer Header (7 bytes)
        buffer.push(self.transaction_id);
        let flags = (if self.is_response { 0x10 } else { 0 })
            | (if self.is_aborted { 0x20 } else { 0 })
            | ((self.segmentation as u8) << 6);
        buffer.push(flags);

        // Valid payload length (data only)
        buffer.push(self.data.len() as u8); // Spec 6.3.3.2: Max 255 bytes

        buffer.push(self.command_id as u8);
        buffer.extend_from_slice(&self.index.to_le_bytes());
        buffer.push(self.sub_index);

        // 3. Data
        buffer.extend_from_slice(&self.data);

        buffer
    }
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

        // Call the inherent method
        let deserialized = PdoSequenceLayerHeader::deserialize(&buffer).unwrap();
        assert_eq!(original, deserialized);
    }

    #[test]
    fn test_pdo_sdo_command_roundtrip() {
        let original = PdoSdoCommand {
            sequence_header: PdoSequenceLayerHeader {
                sequence_number: 5,
                connection_state: 2,
            },
            transaction_id: 3,
            is_response: true,
            is_aborted: false,
            segmentation: Segmentation::Expedited,
            valid_payload_length: 4, // Set by serialize
            command_id: CommandId::ReadByIndex,
            index: 0x1008,
            sub_index: 0,
            data: vec![0xDE, 0xAD, 0xBE, 0xEF],
        };

        let buffer = original.serialize();

        // Seq(1) + TID(1) + Flags(1) + VPL(1) + Cmd(1) + Index(2) + Sub(1) + Data(4) = 12 bytes
        assert_eq!(buffer.len(), 12);
        
        // Check VPL was set correctly
        assert_eq!(buffer[3], 4); // valid_payload_length

        let deserialized = PdoSdoCommand::deserialize(&buffer).unwrap();

        // Update original VPL to match what serialize() would have set
        let mut expected = original;
        expected.valid_payload_length = 4;

        assert_eq!(expected, deserialized);
    }
}