use crate::PowerlinkError;
use crate::frame::basic::{EthernetHeader, MacAddress};
use crate::frame::codec::{Codec, CodecHelpers};
use crate::pdo::PDOVersion;
use crate::types::{C_ADR_MN_DEF_NODE_ID, MessageType, NodeId};
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
    /// Size of the actual payload data in bytes.
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
    /// Serializes the PReq frame into the provided buffer.
    /// Returns the total size of the POWERLINK frame section written,
    /// including padding if necessary to meet minimum Ethernet payload size.
    /// Assumes buffer starts *after* the Ethernet header.
    fn serialize(&self, buffer: &mut [u8]) -> Result<usize, PowerlinkError> {
        let pl_header_size = 10; // MType(1)+Dest(1)+Src(1)+Rsvd(1)+Flags(1)+Rsvd(1)+PDOv(1)+Rsvd(1)+Size(2)
        let total_pl_frame_size = pl_header_size + self.payload.len();
        // Check buffer size for unpadded PL frame first
        if buffer.len() < total_pl_frame_size {
            return Err(PowerlinkError::BufferTooShort);
        }

        // --- Serialize POWERLINK Header ---
        CodecHelpers::serialize_pl_header(self.message_type, self.destination, self.source, buffer);
        buffer[3] = 0; // Reserved

        // PReq Specific Header Fields
        let mut octet4_flags = 0u8;
        if self.flags.ms {
            octet4_flags |= 1 << 5;
        }
        if self.flags.ea {
            octet4_flags |= 1 << 2;
        }
        if self.flags.rd {
            octet4_flags |= 1 << 0;
        }
        buffer[4] = octet4_flags; // Flags byte
        buffer[5] = 0; // Reserved
        buffer[6] = self.pdo_version.0;
        buffer[7] = 0; // Reserved
        buffer[8..10].copy_from_slice(&self.payload_size.to_le_bytes()); // Actual payload size

        // --- Serialize Payload ---
        let payload_start = pl_header_size;
        let payload_end = payload_start + self.payload.len();
        // Bounds already checked for total_pl_frame_size
        buffer[payload_start..payload_end].copy_from_slice(&self.payload);

        // --- Determine Padded Size ---
        let pl_frame_len = payload_end; // Length before padding
        let min_eth_payload = 46; // Minimum Ethernet payload size
        let padded_pl_len = pl_frame_len.max(min_eth_payload);

        // Apply padding if necessary
        if padded_pl_len > pl_frame_len {
            if buffer.len() < padded_pl_len {
                return Err(PowerlinkError::BufferTooShort); // Need space for padding
            }
            buffer[pl_frame_len..padded_pl_len].fill(0); // Pad with zeros
        }

        Ok(padded_pl_len) // Return the total size written, including padding
    }

    /// Deserializes a PReq frame from the provided buffer.
    /// Assumes the buffer starts *after* the 14-byte Ethernet header.
    fn deserialize(eth_header: EthernetHeader, buffer: &[u8]) -> Result<Self, PowerlinkError> {
        let pl_header_size = 10;
        if buffer.len() < pl_header_size {
            // Need at least the header
            return Err(PowerlinkError::BufferTooShort);
        }

        // Deserialize Basic PL Header
        let (message_type, destination, source) = CodecHelpers::deserialize_pl_header(buffer)?;
        // buffer[3] is reserved

        // Validate message type
        if message_type != MessageType::PReq {
            return Err(PowerlinkError::InvalidPlFrame);
        }

        // Deserialize PReq Specific Header Fields
        let octet4_flags = buffer[4];
        let flags = PReqFlags {
            ms: (octet4_flags & (1 << 5)) != 0,
            ea: (octet4_flags & (1 << 2)) != 0,
            rd: (octet4_flags & (1 << 0)) != 0,
        };
        // buffer[5] is reserved
        let pdo_version = PDOVersion(buffer[6]);
        // buffer[7] is reserved
        let payload_size = u16::from_le_bytes(buffer[8..10].try_into()?);

        // Deserialize Payload
        let payload_start = pl_header_size;
        let payload_end = payload_start + payload_size as usize;

        // Check buffer length against the *indicated* payload size
        if buffer.len() < payload_end {
            return Err(PowerlinkError::BufferTooShort);
        }
        // The payload *is* the data up to payload_size. Padding is not part of the payload vec.
        let payload = buffer[payload_start..payload_end].to_vec();

        Ok(Self {
            eth_header, // Use the passed-in Eth header
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
    use crate::frame::codec::CodecHelpers;
    use crate::types::MessageType;
    use alloc::vec; // Need this for tests

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
            MacAddress([0x00, 0x00, 0x00, 0x00, 0x00, 55]), // Example dest MAC
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
            vec![0x01, 0x02, 0x03, 0x04], // Payload len = 4
        );

        let mut buffer = [0u8; 128]; // Buffer for full Ethernet frame
        // 1. Serialize Eth header
        CodecHelpers::serialize_eth_header(&original_frame.eth_header, &mut buffer);
        // 2. Serialize PL frame part
        let pl_bytes_written = original_frame.serialize(&mut buffer[14..]).unwrap();
        // PL Header = 10, Payload = 4. Total = 14. < 46. Padded PL len = 46.
        assert_eq!(pl_bytes_written, 46);
        let total_frame_len = 14 + pl_bytes_written;

        // 3. Deserialize full frame
        let deserialized_frame = crate::frame::deserialize_frame(&buffer[..total_frame_len])
            .unwrap()
            .into_preq()
            .unwrap();

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
            vec![], // Payload len = 0
        );

        let mut buffer = [0u8; 128];
        CodecHelpers::serialize_eth_header(&original_frame.eth_header, &mut buffer);
        let pl_bytes_written = original_frame.serialize(&mut buffer[14..]).unwrap();
        let total_frame_len = 14 + pl_bytes_written;

        // PReq header = 10 bytes. Payload = 0. Total PL Frame = 10 bytes.
        // Min Eth Payload = 46 bytes. Needs padding.
        assert_eq!(pl_bytes_written, 46); // Padded POWERLINK frame section size

        let deserialized_frame = crate::frame::deserialize_frame(&buffer[..total_frame_len])
            .unwrap()
            .into_preq()
            .unwrap();

        assert_eq!(original_frame, deserialized_frame);
        assert!(deserialized_frame.payload.is_empty());
        assert_eq!(deserialized_frame.payload_size, 0); // Check indicated size is 0
    }

    #[test]
    fn test_preq_deserialize_short_buffer() {
        // Test short buffer for header (less than 14 bytes for Eth header)
        let buffer_short_header = [0u8; 13];
        let result_header = crate::frame::deserialize_frame(&buffer_short_header);
        // Correct the assertion: Expect BufferTooShort, not InvalidEthernetFrame
        assert!(matches!(result_header, Err(PowerlinkError::BufferTooShort)));

        // Test buffer that is too short for payload size indicated IN the frame
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
        CodecHelpers::serialize_eth_header(&original_frame.eth_header, &mut long_buffer);
        original_frame.serialize(&mut long_buffer[14..]).unwrap();
        // PL Header = 10, Payload = 100. Total = 110. > 46. No padding needed.
        // Total bytes in buffer = 14 + 110 = 124.

        // Slice the buffer to be long enough for the header, but not the indicated payload.
        // Header = 14 (Eth) + 10 (PL) = 24 total. Indicated payload size = 100. Payload end = 14 + 10 + 100 = 124.
        let short_slice = &long_buffer[..50]; // Slice is 50 bytes long. Header fits, payload doesn't.
        let result_payload = crate::frame::deserialize_frame(short_slice); // Pass full buffer
        // deserialize_frame will pass &short_slice[14..] (len 36) to PReqFrame::deserialize
        // PReqFrame::deserialize will read payload_size=100 from bytes 8-9 (indices 22-23 of full slice)
        // It will check if 36 < 10 + 100, which is true, and return BufferTooShort.
        assert!(matches!(
            result_payload,
            Err(PowerlinkError::BufferTooShort)
        ));
    }
}
