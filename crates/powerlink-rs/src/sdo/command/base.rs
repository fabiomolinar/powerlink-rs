// crates/powerlink-rs/src/sdo/command/base.rs
use crate::PowerlinkError;
use crate::types::{UNSIGNED8, UNSIGNED16, UNSIGNED32};
use alloc::vec::Vec;

/// Defines the SDO command IDs.
/// (Reference: EPSG DS 301, Table 58)
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CommandLayerHeader {
    pub transaction_id: UNSIGNED8,
    pub is_response: bool,
    pub is_aborted: bool,
    pub segmentation: Segmentation,
    pub command_id: CommandId,
    pub segment_size: UNSIGNED16,
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

// SdoCommand is a payload, not a full frame. It does not implement Codec.
impl SdoCommand {
    /// Serializes the SDO command into the provided buffer.
    /// Returns the number of bytes written.
    pub fn serialize(&self, buffer: &mut [u8]) -> Result<usize, PowerlinkError> {
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

    /// Deserializes an SDO command from the provided buffer.
    pub fn deserialize(buffer: &[u8]) -> Result<Self, PowerlinkError> {
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
