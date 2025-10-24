// crates/powerlink-rs/src/frame/control/asnd.rs

use crate::PowerlinkError;
use crate::frame::basic::{ETHERNET_HEADER_SIZE, EthernetHeader, MacAddress};
use crate::frame::codec::{Codec, CodecHelpers};
use crate::types::{MessageType, NodeId};
use alloc::vec::Vec;

/// Service IDs for ASnd frames.
/// (Reference: EPSG DS 301, Appendix 3.3)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ServiceId {
    /// Corresponds to `IDENT_RESPONSE`.
    IdentResponse = 0x01,
    /// Corresponds to `STATUS_RESPONSE`.
    StatusResponse = 0x02,
    /// Corresponds to `NMT_REQUEST`.
    NmtRequest = 0x03,
    /// Corresponds to `NMT_COMMAND`.
    NmtCommand = 0x04,
    /// Corresponds to `SDO`.
    Sdo = 0x05,
}

impl TryFrom<u8> for ServiceId {
    type Error = PowerlinkError;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x01 => Ok(Self::IdentResponse),
            0x02 => Ok(Self::StatusResponse),
            0x03 => Ok(Self::NmtRequest),
            0x04 => Ok(Self::NmtCommand),
            0x05 => Ok(Self::Sdo),
            _ => Err(PowerlinkError::InvalidServiceId(value)),
        }
    }
}

/// Represents a complete ASnd frame.
/// (Reference: EPSG DS 301, Section 4.6.1.1.6)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ASndFrame {
    pub eth_header: EthernetHeader,
    pub message_type: MessageType,
    pub destination: NodeId,
    pub source: NodeId,
    pub service_id: ServiceId,
    pub payload: Vec<u8>,
}

impl ASndFrame {
    /// Creates a new ASnd frame.
    pub fn new(
        source_mac: MacAddress,
        dest_mac: MacAddress,
        target_node_id: NodeId,
        source_node_id: NodeId,
        service_id: ServiceId,
        payload: Vec<u8>,
    ) -> Self {
        let eth_header = EthernetHeader::new(dest_mac, source_mac);

        ASndFrame {
            eth_header,
            message_type: MessageType::ASnd,
            destination: target_node_id,
            source: source_node_id,
            service_id,
            payload,
        }
    }
}

impl Codec for ASndFrame {
    fn serialize(&self, buffer: &mut [u8]) -> Result<usize, PowerlinkError> {
        let header_size = 18; // 4 bytes for PL header
        let total_size = header_size + self.payload.len();
        if buffer.len() < total_size {
            return Err(PowerlinkError::FrameTooLarge);
        }

        CodecHelpers::serialize_eth_header(&self.eth_header, buffer);
        CodecHelpers::serialize_pl_header(self.message_type, self.destination, self.source, buffer);

        buffer[17] = self.service_id as u8;

        // Payload
        buffer[header_size..total_size].copy_from_slice(&self.payload);

        Ok(total_size)
    }

    fn deserialize(buffer: &[u8]) -> Result<Self, PowerlinkError> {
        let header_size = ETHERNET_HEADER_SIZE + 4;
        if buffer.len() < header_size {
            return Err(PowerlinkError::BufferTooShort);
        }

        let eth_header = CodecHelpers::deserialize_eth_header(buffer)?;
        let (message_type, destination, source) = CodecHelpers::deserialize_pl_header(buffer)?;
        let service_id = ServiceId::try_from(buffer[17])?;

        let payload = buffer[header_size..].to_vec();

        Ok(Self {
            eth_header,
            message_type,
            destination,
            source,
            service_id,
            payload,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn test_asnd_codec_roundtrip() {
        let source_mac = MacAddress([0x11; 6]);
        let dest_mac = MacAddress([0x22; 6]);
        let original_frame = ASndFrame::new(
            source_mac,
            dest_mac,
            NodeId(10),
            NodeId(240),
            ServiceId::Sdo,
            vec![0xDE, 0xAD, 0xBE, 0xEF],
        );

        let mut buffer = [0u8; 128];
        let bytes_written = original_frame.serialize(&mut buffer).unwrap();

        let deserialized_frame = ASndFrame::deserialize(&buffer[..bytes_written]).unwrap();

        assert_eq!(original_frame, deserialized_frame);
    }

    #[test]
    fn test_asnd_codec_empty_payload() {
        let source_mac = MacAddress([0xAA; 6]);
        let dest_mac = MacAddress([0xBB; 6]);
        let original_frame = ASndFrame::new(
            source_mac,
            dest_mac,
            NodeId(20),
            NodeId(240),
            ServiceId::StatusResponse,
            vec![],
        );

        let mut buffer = [0u8; 128];
        let bytes_written = original_frame.serialize(&mut buffer).unwrap();
        assert_eq!(bytes_written, 18); // Header only

        let deserialized_frame = ASndFrame::deserialize(&buffer[..bytes_written]).unwrap();

        assert_eq!(original_frame, deserialized_frame);
        assert!(deserialized_frame.payload.is_empty());
    }

    #[test]
    fn test_asnd_deserialize_short_buffer() {
        let buffer = [0u8; 17]; // One byte too short for ASnd header
        let result = ASndFrame::deserialize(&buffer);
        assert!(matches!(result, Err(PowerlinkError::BufferTooShort)));
    }
}
