// In crates/powerlink-rs/src/sdo/sequence.rs

use crate::PowerlinkError;
use crate::frame::Codec;

/// Defines the connection state for the SDO Sequence Layer.
///
/// These values correspond to the `rcon` fields in the header.
/// (Reference: EPSG DS 301, Table 53)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum ReceiveConnState {
    /// No connection established.
    #[default]
    NoConnection = 0,
    /// Connection initialization requested.
    Initialization = 1,
    /// Connection is valid and active.
    ConnectionValid = 2,
    /// An error has occurred, and retransmission is requested.
    ErrorResponse = 3,
}

impl TryFrom<u8> for ReceiveConnState {
    type Error = PowerlinkError;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::NoConnection),
            1 => Ok(Self::Initialization),
            2 => Ok(Self::ConnectionValid),
            3 => Ok(Self::ErrorResponse),
            _ => Err(PowerlinkError::InvalidEnumValue),
        }
    }
}

/// Defines the connection state for the SDO Sequence Layer.
///
/// These values correspond to the `scon` fields in the header.
/// (Reference: EPSG DS 301, Table 53)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum SendConnState {
    /// No connection established.
    #[default]
    NoConnection = 0,
    /// Connection initialization requested.
    Initialization = 1,
    /// Connection is valid and active.
    ConnectionValid = 2,
    /// Connection is valid, and an acknowledgement is requested.
    ConnectionValidAckRequest = 3,
}

impl TryFrom<u8> for SendConnState {
    type Error = PowerlinkError;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::NoConnection),
            1 => Ok(Self::Initialization),
            2 => Ok(Self::ConnectionValid),
            3 => Ok(Self::ConnectionValidAckRequest),
            _ => Err(PowerlinkError::InvalidEnumValue),
        }
    }
}

/// Represents the 4-byte header for the Asynchronous SDO Sequence Layer.
///
/// (Reference: EPSG DS 301, Table 52 and 53)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SequenceLayerHeader {
    pub receive_sequence_number: u8,   // rsnr (0-63)
    pub receive_con: ReceiveConnState, // rcon
    pub send_sequence_number: u8,      // ssnr (0-63)
    pub send_con: SendConnState,       // scon
}

impl Codec for SequenceLayerHeader {
    fn serialize(&self, buffer: &mut [u8]) -> Result<usize, PowerlinkError> {
        const HEADER_SIZE: usize = 4;
        if buffer.len() < HEADER_SIZE {
            return Err(PowerlinkError::BufferTooShort);
        }

        // Octet 0: rcon (2 bits) and rsnr (6 bits)
        buffer[0] = (self.receive_sequence_number << 2) | (self.receive_con as u8);
        // Octet 1: scon (2 bits) and ssnr (6 bits)
        buffer[1] = (self.send_sequence_number << 2) | (self.send_con as u8);
        // Octets 2-3 are reserved
        buffer[2..4].copy_from_slice(&[0, 0]);

        Ok(HEADER_SIZE)
    }

    fn deserialize(buffer: &[u8]) -> Result<Self, PowerlinkError> {
        const HEADER_SIZE: usize = 4;
        if buffer.len() < HEADER_SIZE {
            return Err(PowerlinkError::BufferTooShort);
        }

        let octet0 = buffer[0];
        let octet1 = buffer[1];

        Ok(Self {
            receive_con: ReceiveConnState::try_from(octet0 & 0b11)?,
            receive_sequence_number: (octet0 >> 2) & 0b0011_1111,
            send_con: SendConnState::try_from(octet1 & 0b11)?,
            send_sequence_number: (octet1 >> 2) & 0b0011_1111,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sequence_header_codec_roundtrip() {
        let original_header = SequenceLayerHeader {
            receive_sequence_number: 42,
            receive_con: ReceiveConnState::ConnectionValid,
            send_sequence_number: 15,
            send_con: SendConnState::ConnectionValidAckRequest,
        };

        let mut buffer = [0u8; 4];
        let bytes_written = original_header.serialize(&mut buffer).unwrap();
        assert_eq!(bytes_written, 4);

        // Expected byte values based on the spec's bit layout:
        // Byte 0: rsnr(42=0x2A)<<2 | rcon(2) = 0xA8 | 0x02 = 0xAA
        // Byte 1: ssnr(15=0x0F)<<2 | scon(3) = 0x3C | 0x03 = 0x3F
        assert_eq!(buffer, [0xAA, 0x3F, 0x00, 0x00]);

        let deserialized_header = SequenceLayerHeader::deserialize(&buffer).unwrap();
        assert_eq!(original_header, deserialized_header);
    }

    #[test]
    fn test_conn_state_try_from() {
        assert_eq!(
            ReceiveConnState::try_from(2),
            Ok(ReceiveConnState::ConnectionValid)
        );
        assert!(ReceiveConnState::try_from(4).is_err());
        assert_eq!(
            SendConnState::try_from(3),
            Ok(SendConnState::ConnectionValidAckRequest)
        );
        assert!(SendConnState::try_from(4).is_err());
    }
}
