// crates/powerlink-rs/src/sdo/command/base.rs
use crate::PowerlinkError;
use alloc::string::String;
use alloc::vec::Vec;
use core::convert::TryFrom;

/// SDO Command ID (Byte 0 of SDO Command Layer).
/// (Reference: EPSG DS 301, Table 48)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CommandId {
    /// No Operation (0x00)
    Nil = 0x00,
    /// Write by Index (0x01)
    WriteByIndex = 0x01,
    /// Read by Index (0x02)
    ReadByIndex = 0x02,
    /// Write All by Index (0x03)
    WriteAllByIndex = 0x03,
    /// Read All by Index (0x04)
    ReadAllByIndex = 0x04,
    /// Write by Name (0x05)
    WriteByName = 0x05,
    /// Read by Name (0x06)
    ReadByName = 0x06,
    // File transfer
    FileWrite = 0x20,
    FileRead = 0x21,
    // Variable groups
    WriteMultipleParamByIndex = 0x31,
    ReadMultipleParamByIndex = 0x32,
    /// Abort Transfer (0x40)
    Abort = 0x40,
    // Parameter service
    MaxSegmentSize = 0x70,
    // Manufacturer specific from 0x80 to 0xFF    
}

impl Default for CommandId {
    fn default() -> Self {
        Self::Nil
    }
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
            0x40 => Ok(Self::Abort),
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
    /// Deserializes an SDO command from a buffer.
    /// Assumes the buffer starts *at* the Command Layer (after the Sequence Layer).
    /// (Reference: EPSG DS 301, Section 6.3.2.4.1, Table 54)
    pub fn deserialize(buffer: &[u8]) -> Result<Self, PowerlinkError> {
        // Fixed command header is 8 bytes
        // (Reserved, TID, Flags, CommandID, SegSize, Reserved)
        const FIXED_HEADER_SIZE: usize = 8;
        if buffer.len() < FIXED_HEADER_SIZE {
            return Err(PowerlinkError::BufferTooShort);
        }

        // buffer[0] is reserved
        let transaction_id = buffer[1];
        let flags = buffer[2];
        let is_response = (flags & 0x10) != 0;
        let is_aborted = (flags & 0x20) != 0;
        let segmentation = Segmentation::try_from((flags >> 6) & 0b11)?;

        let command_id = CommandId::try_from(buffer[3])?;
        // Segment Size (bytes 4-5)
        let segment_size = u16::from_le_bytes(buffer[4..6].try_into()?);
        // buffer[6..8] is reserved

        let header = CommandLayerHeader {
            transaction_id,
            is_response,
            is_aborted,
            segmentation,
            command_id,
            segment_size,
        };

        // If it's an Initiate frame, the next 4 bytes are the total data size.
        // (Reference: EPSG DS 301, Section 6.3.2.4.1, Table 54)
        let (data_size, payload_offset) = if segmentation == Segmentation::Initiate {
            const INITIATE_HEADER_SIZE: usize = 12;
            if buffer.len() < INITIATE_HEADER_SIZE {
                return Err(PowerlinkError::BufferTooShort);
            }
            let size = u32::from_le_bytes(buffer[8..12].try_into()?);
            (Some(size), INITIATE_HEADER_SIZE)
        } else {
            (None, FIXED_HEADER_SIZE)
        };

        // The payload starts after the fixed header (and data size, if present)
        // The segment_size field indicates the length of this payload.
        let payload_end = payload_offset + (segment_size as usize);
        if buffer.len() < payload_end {
            return Err(PowerlinkError::BufferTooShort);
        }
        let payload = buffer[payload_offset..payload_end].to_vec();

        Ok(SdoCommand {
            header,
            data_size,
            payload,
        })
    }

    /// Serializes the SDO command into the provided buffer.
    /// Assumes buffer starts *at* the Command Layer.
    /// Returns the total number of bytes written (header + payload).
    pub fn serialize(&self, buffer: &mut [u8]) -> Result<usize, PowerlinkError> {
        let flags = (if self.header.is_response { 0x10 } else { 0 })
            | (if self.header.is_aborted { 0x20 } else { 0 })
            | ((self.header.segmentation as u8) << 6);

        // Fixed command header is 8 bytes
        const FIXED_HEADER_SIZE: usize = 8;
        if buffer.len() < FIXED_HEADER_SIZE {
            return Err(PowerlinkError::BufferTooShort);
        }

        buffer[0] = 0; // Reserved
        buffer[1] = self.header.transaction_id;
        buffer[2] = flags;
        buffer[3] = self.header.command_id as u8;
        buffer[4..6].copy_from_slice(&self.header.segment_size.to_le_bytes());
        buffer[6..8].copy_from_slice(&[0, 0]); // Reserved

        let mut current_offset = FIXED_HEADER_SIZE;

        if self.header.segmentation == Segmentation::Initiate {
            const INITIATE_HEADER_SIZE: usize = 12;
            if buffer.len() < INITIATE_HEADER_SIZE {
                return Err(PowerlinkError::BufferTooShort);
            }
            let size = self.data_size.unwrap_or(0);
            buffer[current_offset..current_offset + 4].copy_from_slice(&size.to_le_bytes());
            current_offset += 4;
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
        // Spec 6.3.2.4.2.1.2 (Table 61) shows:
        // Index (2 bytes), Sub-Index (1 byte), reserved (1 byte)
        // Total 4 bytes.
        if payload.len() < 4 {
            return Err(PowerlinkError::SdoInvalidCommandPayload);
        }
        Ok(Self {
            index: u16::from_le_bytes(payload[0..2].try_into()?),
            sub_index: payload[2],
            // Ignore payload[3] (reserved)
        })
    }
}

/// Payload for a ReadByName command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadByNameRequest {
    pub name: String,
}

impl ReadByNameRequest {
    pub fn from_payload(payload: &[u8]) -> Result<Self, PowerlinkError> {
        // The name is the entire payload, potentially zero-terminated.
        let name_end = payload
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(payload.len());
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
        // Spec 6.3.2.4.2.1.1 (Table 59) shows:
        // Index (2 bytes), Sub-Index (1 byte), reserved (1 byte)
        // Total 4 bytes before payload data starts.
        if payload.len() < 4 {
            return Err(PowerlinkError::SdoInvalidCommandPayload);
        }
        Ok(Self {
            index: u16::from_le_bytes(payload[0..2].try_into()?),
            sub_index: payload[2],
            // Ignore payload[3] (reserved)
            data: &payload[4..],
        })
    }
}

/// Payload for a WriteAllByIndex command.
/// (Reference: EPSG DS 301, Section 6.3.2.4.2.1.3, Table 63)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // This struct is optional and not used by the core crate
pub struct WriteAllByIndexRequest<'a> {
    pub index: u16,
    pub data: &'a [u8],
}

#[allow(dead_code)] // This impl is optional and not used by the core crate
impl<'a> WriteAllByIndexRequest<'a> {
    pub fn from_payload(payload: &'a [u8]) -> Result<Self, PowerlinkError> {
        // Spec 6.3.2.4.2.1.3 (Table 63) shows:
        // Index (2 bytes), reserved (1 byte), reserved (1 byte)
        // Total 4 bytes before payload data starts.
        if payload.len() < 4 {
            return Err(PowerlinkError::SdoInvalidCommandPayload);
        }
        Ok(Self {
            index: u16::from_le_bytes(payload[0..2].try_into()?),
            // Ignore payload[2] and payload[3] (reserved)
            data: &payload[4..],
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteByNameRequest<'a> {
    pub name: String,
    pub data: &'a [u8],
}

impl<'a> WriteByNameRequest<'a> {
    pub fn from_payload(payload: &'a [u8]) -> Result<Self, PowerlinkError> {
        // TODO: This implementation does not match Table 67/68,
        // which describe a complex payload with an internal offset.
        // For now, assume simple zero-terminated string + data.
        let name_end = payload.iter().position(|&b| b == 0).unwrap_or(0);
        if name_end == 0 {
            return Err(PowerlinkError::SdoInvalidCommandPayload);
        }
        let name = String::from_utf8(payload[..name_end].to_vec())
            .map_err(|_| PowerlinkError::SdoInvalidCommandPayload)?;
        let data = &payload[name_end + 1..];
        Ok(Self { name, data })
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
        // Spec 6.3.2.4.2.3.5 (Table 79) shows:
        // Index (2 bytes), Sub-Index (1 byte), reserved (1 byte)
        // Total 4 bytes per entry.
        if payload.len() % 4 != 0 {
            return Err(PowerlinkError::SdoInvalidCommandPayload);
        }
        let entries = payload
            .chunks_exact(4)
            .map(|chunk| MultipleParamEntry {
                index: u16::from_le_bytes(chunk[0..2].try_into().unwrap()),
                sub_index: chunk[2],
                // Ignore chunk[3] (reserved)
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
        // Corrected size: 8 (fixed header) + 8 (payload) = 16
        assert_eq!(bytes_written, 16);

        // Call the inherent method, not a trait method
        let deserialized = SdoCommand::deserialize(&buffer[..bytes_written]).unwrap();

        // Check header fields individually
        assert_eq!(
            original.header.transaction_id,
            deserialized.header.transaction_id
        );
        assert_eq!(original.header.is_response, deserialized.header.is_response);
        assert_eq!(original.header.is_aborted, deserialized.header.is_aborted);
        assert_eq!(
            original.header.segmentation,
            deserialized.header.segmentation
        );
        assert_eq!(original.header.command_id, deserialized.header.command_id);
        // segment_size in the header is the length of the data *in that segment*
        // which is just the payload for expedited.
        assert_eq!(original.header.segment_size, 8);
        assert_eq!(deserialized.header.segment_size, 8);

        // Check payload
        assert_eq!(original.data_size, deserialized.data_size);
        assert_eq!(original.payload, deserialized.payload);
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
                segment_size: 4, // Size of payload (index/subindex)
            },
            data_size: Some(1000), // Total size of data to be written
            payload: vec![0x10, 0x60, 0x00, 0x00],
        };

        let mut buffer = [0u8; 64];
        let bytes_written = original.serialize(&mut buffer).unwrap();
        // Corrected size: 8 (fixed header) + 4 (data size) + 4 (payload) = 16
        assert_eq!(bytes_written, 16);

        // Call the inherent method
        let deserialized = SdoCommand::deserialize(&buffer[..bytes_written]).unwrap();

        assert_eq!(original, deserialized);
        assert_eq!(deserialized.data_size, Some(1000));
        assert_eq!(deserialized.header.segment_size, 4);
    }

    #[test]
    fn test_enum_try_from() {
        assert_eq!(CommandId::try_from(0x01), Ok(CommandId::WriteByIndex));
        assert!(CommandId::try_from(0xFF).is_err());
        assert_eq!(Segmentation::try_from(0x02), Ok(Segmentation::Segment));
        assert!(Segmentation::try_from(0x04).is_err());
    }
}
