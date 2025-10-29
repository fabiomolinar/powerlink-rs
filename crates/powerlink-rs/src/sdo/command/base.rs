// crates/powerlink-rs/src/sdo/command/base.rs
use crate::PowerlinkError;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::convert::TryFrom;

/// Represents the SDO command identifier.
/// (Reference: EPSG DS 301, Section 6.3.2.1, Table 100)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum CommandId {
    #[default]
    Nil = 0x00,
    // SDO protocol
    WriteByIndex = 0x01,
    ReadByIndex = 0x02,
    WriteAllByIndex = 0x03,
    ReadAllByIndex = 0x04,
    WriteByName = 0x05,
    ReadByName = 0x06,
    // File transfer
    FileWrite = 0x20,
    FileRead = 0x21,
    // Variable groups
    WriteMultipleParamByIndex = 0x31,
    ReadMultipleParamByIndex = 0x32,
    // Parameter service
    MaxSegmentSize = 0x70,
    // Manufacturer specific from 0x80 to 0xFF
}

impl TryFrom<u8> for CommandId {
    type Error = PowerlinkError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x00 => Ok(Self::Nil),
            0x01 => Ok(Self::WriteByIndex),
            0x02 => Ok(Self::ReadByIndex),
            0x03 => Ok(Self::WriteAllByIndex),
            0x04 => Ok(Self::ReadAllByIndex),
            0x05 => Ok(Self::WriteByName),
            0x06 => Ok(Self::ReadByName),
            0x20 => Ok(Self::FileWrite),
            0x21 => Ok(Self::FileRead),
            0x31 => Ok(Self::WriteMultipleParamByIndex),
            0x32 => Ok(Self::ReadMultipleParamByIndex),
            0x70 => Ok(Self::MaxSegmentSize),
            _ => Err(PowerlinkError::InvalidEnumValue),
        }
    }
}

/// Represents the segmentation type of an SDO transfer.
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

/// Represents the fixed 4-byte header of the SDO Command Layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CommandLayerHeader {
    pub transaction_id: u8,
    pub is_response: bool,
    pub is_aborted: bool,
    pub segmentation: Segmentation,
    pub command_id: CommandId,
    pub segment_size: u16,
}

/// Represents a complete SDO command, including its header and payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdoCommand {
    pub header: CommandLayerHeader,
    // Total size of the data, only present in Initiate frames.
    pub data_size: Option<u32>,
    pub payload: Vec<u8>,
}

impl SdoCommand {
    pub fn deserialize(buffer: &[u8]) -> Result<Self, PowerlinkError> {
        if buffer.len() < 4 {
            return Err(PowerlinkError::BufferTooShort);
        }
        let flags = buffer[0];
        let transaction_id = flags & 0x0F;
        let is_response = (flags & 0x10) != 0;
        let is_aborted = (flags & 0x20) != 0;
        let segmentation = match (flags >> 6) & 0b11 {
            0 => Segmentation::Expedited,
            1 => Segmentation::Initiate,
            2 => Segmentation::Segment,
            3 => Segmentation::Complete,
            _ => unreachable!(),
        };

        let command_id = CommandId::try_from(buffer[1])?;
        let segment_size = u16::from_le_bytes(buffer[2..4].try_into()?);

        let header = CommandLayerHeader {
            transaction_id,
            is_response,
            is_aborted,
            segmentation,
            command_id,
            segment_size,
        };

        // If it's an Initiate frame, the next 4 bytes are the total data size.
        let (data_size, payload_offset) = if segmentation == Segmentation::Initiate {
            if buffer.len() < 8 {
                return Err(PowerlinkError::BufferTooShort);
            }
            let size = u32::from_le_bytes(buffer[4..8].try_into()?);
            (Some(size), 8)
        } else {
            (None, 4)
        };

        // The rest of the buffer is the payload.
        let payload = buffer[payload_offset..].to_vec();

        Ok(SdoCommand {
            header,
            data_size,
            payload,
        })
    }

    pub fn serialize(&self, buffer: &mut [u8]) -> Result<usize, PowerlinkError> {
        let flags = (self.header.transaction_id & 0x0F)
            | if self.header.is_response { 0x10 } else { 0 }
            | if self.header.is_aborted { 0x20 } else { 0 }
            | ((match self.header.segmentation {
                Segmentation::Expedited => 0,
                Segmentation::Initiate => 1,
                Segmentation::Segment => 2,
                Segmentation::Complete => 3,
            }) << 6);

        buffer[0] = flags;
        buffer[1] = self.header.command_id as u8;
        buffer[2..4].copy_from_slice(&self.header.segment_size.to_le_bytes());

        let mut current_offset = 4;

        if self.header.segmentation == Segmentation::Initiate {
            if let Some(size) = self.data_size {
                buffer[current_offset..current_offset + 4].copy_from_slice(&size.to_le_bytes());
                current_offset += 4;
            }
        }

        let payload_len = self.payload.len();
        if buffer.len() < current_offset + payload_len {
            return Err(PowerlinkError::BufferTooShort);
        }
        buffer[current_offset..current_offset + payload_len].copy_from_slice(&self.payload);

        Ok(current_offset + payload_len)
    }
}

// --- Request Payload Structures ---

/// Payload for a ReadByIndex or WriteByIndex command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadByIndexRequest {
    pub index: u16,
    pub sub_index: u8,
}

impl ReadByIndexRequest {
    pub fn from_payload(payload: &[u8]) -> Result<Self, PowerlinkError> {
        if payload.len() < 3 {
            return Err(PowerlinkError::SdoInvalidCommandPayload);
        }
        Ok(Self {
            index: u16::from_le_bytes(payload[0..2].try_into()?),
            sub_index: payload[2],
        })
    }
}

/// Payload for a ReadByName or WriteByName command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadByNameRequest {
    pub name: String,
}

impl ReadByNameRequest {
    pub fn from_payload(payload: &[u8]) -> Result<Self, PowerlinkError> {
        // The name is a zero-terminated string.
        let name_end = payload.iter().position(|&b| b == 0).unwrap_or(payload.len());
        let name = String::from_utf8(payload[..name_end].to_vec())
            .map_err(|_| PowerlinkError::SdoInvalidCommandPayload)?;
        Ok(Self { name })
    }
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteByIndexRequest<'a> {
    pub index: u16,
    pub sub_index: u8,
    pub data: &'a [u8],
}

impl<'a> WriteByIndexRequest<'a> {
    pub fn from_payload(payload: &'a [u8]) -> Result<Self, PowerlinkError> {
        if payload.len() < 3 {
            return Err(PowerlinkError::SdoInvalidCommandPayload);
        }
        Ok(Self {
            index: u16::from_le_bytes(payload[0..2].try_into()?),
            sub_index: payload[2],
            data: &payload[3..],
        })
    }
}

/// A single entry in a Read/Write Multiple Parameters request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MultipleParamEntry {
    pub index: u16,
    pub sub_index: u8,
}

/// Payload for a ReadMultipleParamByIndex request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadMultipleParamRequest {
    pub entries: Vec<MultipleParamEntry>,
}

impl ReadMultipleParamRequest {
    pub fn from_payload(payload: &[u8]) -> Result<Self, PowerlinkError> {
        if payload.len() % 3 != 0 {
            return Err(PowerlinkError::SdoInvalidCommandPayload);
        }
        let entries = payload
            .chunks_exact(3)
            .map(|chunk| MultipleParamEntry {
                index: u16::from_le_bytes(chunk[0..2].try_into().unwrap()),
                sub_index: chunk[2],
            })
            .collect();
        Ok(Self { entries })
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
        // Call the inherent method, not a trait method
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
        // Call the inherent method
        let deserialized = SdoCommand::deserialize(&buffer[..bytes_written]).unwrap();

        assert_eq!(original, deserialized);
        assert_eq!(deserialized.data_size, Some(1000));
    }

    #[test]
    fn test_enum_try_from() {
        assert_eq!(CommandId::try_from(0x01), Ok(CommandId::WriteByIndex));
        assert!(CommandId::try_from(0xFF).is_err());
        assert_eq!(Segmentation::try_from(0x02), Ok(Segmentation::Segment));
        assert!(Segmentation::try_from(0x04).is_err());
    }
}
