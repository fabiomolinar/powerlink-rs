// crates/powerlink-rs/src/frame/poll/preq.rs

use crate::frame::basic::{EthernetHeader, MacAddress};
use crate::frame::codec::{Codec, CodecHelpers};
use crate::pdo::PDOVersion;
use crate::types::{MessageType, NodeId, C_ADR_MN_DEF_NODE_ID};
use crate::PowerlinkError;
use alloc::vec::Vec;

/// Represents a Poll Request frame (MN unicast frame to CN).
/// (EPSG DS 301, Section 4.6.1.1.3)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PReqFrame {
    pub eth_header: EthernetHeader,
    pub message_type: MessageType,
    pub destination: NodeId,
    pub source: NodeId,
    pub flags: PReqFlags,
    pub pdo_version: PDOVersion,
    pub payload_size: u16,
    pub payload: Vec<u8>,
}

/// Flags specific to the PReq frame.
/// (EPSG DS 301, Table 18)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PReqFlags {
    pub ms: bool, // Multiplexed Slot
    pub ea: bool, // Exception Acknowledge
    pub rd: bool, // Ready
}

impl PReqFrame {
    /// Creates a PReq frame destined for a specific Controlled Node.
    pub fn new(
        source_mac: MacAddress,
        dest_mac: MacAddress,
        target_node_id: NodeId,
        flags: PReqFlags,
        pdo_version: PDOVersion,
        payload: Vec<u8>,
    ) -> Self {
        let eth_header = EthernetHeader::new(dest_mac, source_mac);
        let payload_size = payload.len() as u16;

        PReqFrame {
            eth_header,
            message_type: MessageType::PReq,
            destination: target_node_id,
            source: NodeId(C_ADR_MN_DEF_NODE_ID),
            flags,
            pdo_version,
            payload_size,
            payload,
        }
    }
}

impl Codec for PReqFrame {
    fn serialize(&self, buffer: &mut [u8]) -> Result<usize, PowerlinkError> {
        let header_size = 24;
        let total_size = header_size + self.payload.len();
        if buffer.len() < total_size {
            return Err(PowerlinkError::BufferTooShort);
        }

        CodecHelpers::serialize_eth_header(&self.eth_header, buffer);
        CodecHelpers::serialize_pl_header(self.message_type, self.destination, self.source, buffer);

        buffer[17] = 0;
        let mut octet4 = 0u8;
        if self.flags.ms {
            octet4 |= 1 << 5;
        }
        if self.flags.ea {
            octet4 |= 1 << 2;
        }
        if self.flags.rd {
            octet4 |= 1 << 0;
        }
        buffer[18] = octet4;
        buffer[19] = 0;
        buffer[20] = self.pdo_version.0;
        buffer[21] = 0;
        buffer[22..24].copy_from_slice(&self.payload_size.to_le_bytes());
        buffer[header_size..total_size].copy_from_slice(&self.payload);

        Ok(total_size.max(60))
    }

    fn deserialize(buffer: &[u8]) -> Result<Self, PowerlinkError> {
        let header_size = 24;
        if buffer.len() < header_size {
            return Err(PowerlinkError::BufferTooShort);
        }

        let eth_header = CodecHelpers::deserialize_eth_header(buffer)?;
        let (message_type, destination, source) = CodecHelpers::deserialize_pl_header(buffer)?;

        let octet4 = buffer[18];
        let flags = PReqFlags {
            ms: (octet4 & (1 << 5)) != 0,
            ea: (octet4 & (1 << 2)) != 0,
            rd: (octet4 & (1 << 0)) != 0,
        };

        let pdo_version = PDOVersion(buffer[20]);
        let payload_size = u16::from_le_bytes(buffer[22..24].try_into()?);

        let payload_end = header_size + payload_size as usize;
        if buffer.len() < payload_end {
            return Err(PowerlinkError::BufferTooShort);
        }
        let payload = buffer[header_size..payload_end].to_vec();

        Ok(Self {
            eth_header,
            message_type,
            destination,
            source,
            flags,
            pdo_version,
            payload_size,
            payload,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::MessageType;
    use alloc::vec;

    #[test]
    fn test_preqframe_new_constructor() {
        let source_mac = MacAddress([0xAA; 6]);
        let target_node = NodeId(55);
        let payload = vec![0x01, 0x02, 0x03];
        let flags = PReqFlags {
            ms: true,
            ea: false,
            rd: true,
        };
        let frame = PReqFrame::new(
            source_mac,
            MacAddress([0x00, 0x00, 0x00, 0x00, 0x00, 55]),
            target_node,
            flags,
            PDOVersion(1),
            payload.clone(),
        );

        let expected_dest_mac = [0x00, 0x00, 0x00, 0x00, 0x00, 55];
        assert_eq!(frame.eth_header.destination_mac.0, expected_dest_mac);
        assert_eq!(frame.eth_header.source_mac, source_mac);
        assert_eq!(frame.message_type, MessageType::PReq);
        assert_eq!(frame.source, NodeId(C_ADR_MN_DEF_NODE_ID));
        assert_eq!(frame.destination, target_node);
        assert_eq!(frame.payload, payload);
        assert!(frame.flags.rd);
    }

    #[test]
    fn test_preq_codec_roundtrip() {
        let original_frame = PReqFrame::new(
            MacAddress([0xAA; 6]),
            MacAddress([0xBB; 6]),
            NodeId(55),
            PReqFlags {
                ms: true,
                ea: false,
                rd: true,
            },
            PDOVersion(2),
            vec![0x01, 0x02, 0x03, 0x04],
        );

        let mut buffer = [0u8; 128];
        let bytes_written = original_frame.serialize(&mut buffer).unwrap();

        let deserialized_frame = PReqFrame::deserialize(&buffer[..bytes_written]).unwrap();

        assert_eq!(original_frame, deserialized_frame);
    }

    #[test]
    fn test_preq_codec_empty_payload() {
        let original_frame = PReqFrame::new(
            MacAddress([0xAA; 6]),
            MacAddress([0xBB; 6]),
            NodeId(55),
            PReqFlags {
                ms: false,
                ea: true,
                rd: false,
            },
            PDOVersion(3),
            vec![],
        );

        let mut buffer = [0u8; 128];
        let bytes_written = original_frame.serialize(&mut buffer).unwrap();
        assert_eq!(bytes_written, 60); // Padded to min ethernet size

        let deserialized_frame = PReqFrame::deserialize(&buffer[..bytes_written]).unwrap();

        assert_eq!(original_frame, deserialized_frame);
        assert!(deserialized_frame.payload.is_empty());
    }

    #[test]
    fn test_preq_deserialize_short_buffer() {
        // Test short buffer for header
        let buffer = [0u8; 23];
        let result = PReqFrame::deserialize(&buffer);
        assert!(matches!(result, Err(PowerlinkError::BufferTooShort)));

        // Test buffer that is too short for payload
        let original_frame = PReqFrame::new(
            MacAddress([0xAA; 6]),
            MacAddress([0xBB; 6]),
            NodeId(55),
            PReqFlags {
                ms: true,
                ea: false,
                rd: true,
            },
            PDOVersion(2),
            vec![0x01; 100], // Payload of 100 bytes
        );

        let mut long_buffer = [0u8; 200];
        original_frame.serialize(&mut long_buffer).unwrap();

        // Slice the buffer to be long enough for the header, but not the payload.
        let short_slice = &long_buffer[..50]; // Header=24, payload=100, total_len=124. Slice is 50.
        let result_payload = PReqFrame::deserialize(short_slice);
        assert!(matches!(
            result_payload,
            Err(PowerlinkError::BufferTooShort)
        ));
    }
}