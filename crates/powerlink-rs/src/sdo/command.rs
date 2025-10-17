// In crates/powerlink-rs/src/sdo/command.rs
use crate::frame::Codec;
use crate::types::{UNSIGNED16, UNSIGNED32, UNSIGNED8};
use crate::PowerlinkError;
use alloc::vec::Vec;

/// Defines the SDO command IDs.
/// (Reference: EPSG DS 301, Table 58)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CommandId {
    Nil = 0x00,
    WriteByIndex = 0x01,
    ReadByIndex = 0x02,
    // Other commands can be added here later.
}

impl Default for CommandId {
    fn default() -> Self {
        CommandId::Nil
    }
}

impl TryFrom<u8> for CommandId {
    type Error = PowerlinkError;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x00 => Ok(Self::Nil),
            0x01 => Ok(Self::WriteByIndex),
            0x02 => Ok(Self::ReadByIndex),
            _ => Err(PowerlinkError::InvalidEnumValue),
        }
    }
}

/// Defines the segmentation type for SDO transfers.
/// (Reference: EPSG DS 301, Table 55)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum Segmentation {
    #[default]
    Expedited = 0,
    Initiate = 1,
    Segment = 2,
    Complete = 3,
}

impl TryFrom<u8> for Segmentation {
    type Error = PowerlinkError;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Expedited),
            1 => Ok(Self::Initiate),
            2 => Ok(Self::Segment),
            3 => Ok(Self::Complete),
            _ => Err(PowerlinkError::InvalidEnumValue),
        }
    }
}

/// Represents the fixed part of the SDO Command Layer header.
/// (Reference: EPSG DS 301, Table 54)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandLayerHeader {
    pub transaction_id: UNSIGNED8,
    pub is_response: bool,
    pub is_aborted: bool,
    pub segmentation: Segmentation,
    pub command_id: CommandId,
    pub segment_size: UNSIGNED16,
}

impl Default for CommandLayerHeader {
    fn default() -> Self {
        Self {
            transaction_id: 0,
            is_response: false,
            is_aborted: false,
            segmentation: Segmentation::default(),
            command_id: CommandId::default(),
            segment_size: 0,
        }
    }
}

/// Represents a complete SDO command layer frame, including the header and payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdoCommand {
    pub header: CommandLayerHeader,
    /// Data size is only present in Initiate Segmented Transfer frames.
    pub data_size: Option<UNSIGNED32>,
    /// The payload, which contains command-specific data.
    pub payload: Vec<u8>,
}

/// Specific data for a "Read by Index" request.
/// (Reference: EPSG DS 301, Table 61)
pub struct ReadByIndexRequest {
    pub index: u16,
    pub sub_index: u8,
}

impl ReadByIndexRequest {
    pub fn from_payload(payload: &[u8]) -> Result<Self, PowerlinkError> {
        if payload.len() < 4 {
            return Err(PowerlinkError::BufferTooShort);
        }
        Ok(Self {
            index: u16::from_le_bytes(payload[0..2].try_into()?),
            sub_index: payload[2],
        })
    }
}

/// Specific data for a "Write by Index" request.
/// (Reference: EPSG DS 301, Table 59)
pub struct WriteByIndexRequest<'a> {
    pub index: u16,
    pub sub_index: u8,
    pub data: &'a [u8],
}

impl<'a> WriteByIndexRequest<'a> {
    pub fn from_payload(payload: &'a [u8]) -> Result<Self, PowerlinkError> {
        if payload.len() < 4 {
            return Err(PowerlinkError::BufferTooShort);
        }
        Ok(Self {
            index: u16::from_le_bytes(payload[0..2].try_into()?),
            sub_index: payload[2],
            data: &payload[4..],
        })
    }
}

impl Codec for SdoCommand {
    fn serialize(&self, buffer: &mut [u8]) -> Result<usize, PowerlinkError> {
        let mut offset = 0;
        if buffer.len() < 8 {
            return Err(PowerlinkError::BufferTooShort);
        }

        buffer[offset] = 0; // Reserved
        offset += 1;
        buffer[offset] = self.header.transaction_id;
        offset += 1;

        let mut flags = 0u8;
        if self.header.is_response {
            flags |= 1 << 7;
        }
        if self.header.is_aborted {
            flags |= 1 << 6;
        }
        flags |= (self.header.segmentation as u8) << 4;
        buffer[offset] = flags;
        offset += 1;

        buffer[offset] = self.header.command_id as u8;
        offset += 1;
        buffer[offset..offset + 2].copy_from_slice(&self.header.segment_size.to_le_bytes());
        offset += 2;
        buffer[offset..offset + 2].copy_from_slice(&[0, 0]); // Reserved
        offset += 2;

        if let Some(data_size) = self.data_size {
            if buffer.len() < offset + 4 {
                return Err(PowerlinkError::BufferTooShort);
            }
            buffer[offset..offset + 4].copy_from_slice(&data_size.to_le_bytes());
            offset += 4;
        }

        if buffer.len() < offset + self.payload.len() {
            return Err(PowerlinkError::BufferTooShort);
        }
        buffer[offset..offset + self.payload.len()].copy_from_slice(&self.payload);
        offset += self.payload.len();

        Ok(offset)
    }

    fn deserialize(buffer: &[u8]) -> Result<Self, PowerlinkError> {
        if buffer.len() < 8 {
            return Err(PowerlinkError::BufferTooShort);
        }

        let transaction_id = buffer[1];
        let flags = buffer[2];
        let is_response = (flags & (1 << 7)) != 0;
        let is_aborted = (flags & (1 << 6)) != 0;
        let segmentation = Segmentation::try_from((flags >> 4) & 0b11)?;
        let command_id = CommandId::try_from(buffer[3])?;
        let segment_size = u16::from_le_bytes(buffer[4..6].try_into()?);

        let mut offset = 8;
        let data_size = if segmentation == Segmentation::Initiate {
            if buffer.len() < 12 {
                return Err(PowerlinkError::BufferTooShort);
            }
            offset = 12;
            Some(u32::from_le_bytes(buffer[8..12].try_into()?))
        } else {
            None
        };

        let payload = buffer[offset..].to_vec();

        Ok(SdoCommand {
            header: CommandLayerHeader {
                transaction_id,
                is_response,
                is_aborted,
                segmentation,
                command_id,
                segment_size,
            },
            data_size,
            payload,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn test_sdo_command_expedited_roundtrip() {
        let original = SdoCommand {
            header: CommandLayerHeader {
                transaction_id: 1,
                is_response: false,
                is_aborted: false,
                segmentation: Segmentation::Expedited,
                command_id: CommandId::WriteByIndex,
                segment_size: 8,
            },
            data_size: None,
            payload: vec![0x10, 0x18, 0x01, 0x00, 0xDE, 0xAD, 0xBE, 0xEF],
        };

        let mut buffer = [0u8; 64];
        let bytes_written = original.serialize(&mut buffer).unwrap();
        let deserialized = SdoCommand::deserialize(&buffer[..bytes_written]).unwrap();

        assert_eq!(original, deserialized);
    }

    #[test]
    fn test_sdo_command_segmented_initiate_roundtrip() {
        let original = SdoCommand {
            header: CommandLayerHeader {
                transaction_id: 2,
                is_response: false,
                is_aborted: false,
                segmentation: Segmentation::Initiate,
                command_id: CommandId::WriteByIndex,
                segment_size: 4,
            },
            data_size: Some(1000),
            payload: vec![0x10, 0x60, 0x00, 0x00],
        };

        let mut buffer = [0u8; 64];
        let bytes_written = original.serialize(&mut buffer).unwrap();
        let deserialized = SdoCommand::deserialize(&buffer[..bytes_written]).unwrap();

        assert_eq!(original, deserialized);
        assert_eq!(deserialized.data_size, Some(1000));
    }

    #[test]
    fn test_read_by_index_request_parser() {
        let payload = vec![0x06, 0x10, 0x01, 0x00]; // Read 0x1006 sub 1
        let req = ReadByIndexRequest::from_payload(&payload).unwrap();
        assert_eq!(req.index, 0x1006);
        assert_eq!(req.sub_index, 1);

        let short_payload = vec![0x06, 0x10, 0x01];
        assert!(ReadByIndexRequest::from_payload(&short_payload).is_err());
    }

    #[test]
    fn test_write_by_index_request_parser() {
        let payload = vec![0x00, 0x60, 0x00, 0x00, 0xAA, 0xBB]; // Write 0x6000 sub 0 with data
        let req = WriteByIndexRequest::from_payload(&payload).unwrap();
        assert_eq!(req.index, 0x6000);
        assert_eq!(req.sub_index, 0);
        assert_eq!(req.data, &[0xAA, 0xBB]);
    }

    #[test]
    fn test_enum_try_from() {
        assert_eq!(CommandId::try_from(0x01), Ok(CommandId::WriteByIndex));
        assert!(CommandId::try_from(0xFF).is_err());
        assert_eq!(
            Segmentation::try_from(0x02),
            Ok(Segmentation::Segment)
        );
        assert!(Segmentation::try_from(0x04).is_err());
    }
}