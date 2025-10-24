// crates/powerlink-rs/src/frame/poll/pres.rs

use crate::PowerlinkError;
use crate::frame::basic::{EthernetHeader, MacAddress};
use crate::frame::codec::{Codec, CodecHelpers};
use crate::nmt::states::NmtState;
use crate::pdo::PDOVersion;
use crate::types::{C_ADR_BROADCAST_NODE_ID, C_DLL_MULTICAST_PRES, MessageType, NodeId};
use alloc::vec::Vec;

/// A newtype wrapper for the 3-bit RS (Request to Send) flag.
/// (EPSG DS 301, Section 4.2.4.1.2.2)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RSFlag(u8);

impl RSFlag {
    /// Creates a new RSFlag, clamping the value to the valid 0-7 range.
    pub fn new(value: u8) -> Self {
        RSFlag(value.min(7))
    }

    /// Provides read-only access to the underlying u8 value.
    pub fn get(&self) -> u8 {
        self.0
    }
}

/// An enum for the 3-bit PR (Priority) flag.
/// (EPSG DS 301, Appendix 3.2)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PRFlag {
    PrioNmtRequest = 0b111,
    High3 = 0b110,
    High2 = 0b101,
    High1 = 0b100,
    PrioGenericRequest = 0b011,
    Low3 = 0b010,
    Low2 = 0b001,
    Low1 = 0b000,
}

impl Default for PRFlag {
    fn default() -> Self {
        PRFlag::Low1
    }
}

/// Represents a Poll Response frame (CN multicast frame).
/// (EPSG DS 301, Section 4.6.1.1.4)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PResFrame {
    pub eth_header: EthernetHeader,
    pub message_type: MessageType,
    pub destination: NodeId,
    pub source: NodeId,
    pub nmt_state: NmtState,
    pub flags: PResFlags,
    pub pdo_version: PDOVersion,
    pub payload_size: u16,
    pub payload: Vec<u8>,
}

/// Flags specific to the PRes frame.
/// (EPSG DS 301, Table 20)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PResFlags {
    pub ms: bool, // Multiplexed Slot
    pub en: bool, // Exception New
    pub rd: bool, // Ready
    pub pr: PRFlag,
    pub rs: RSFlag,
}

impl PResFrame {
    /// Creates a new PRes frame.
    pub fn new(
        source_mac: MacAddress,
        source_node_id: NodeId,
        nmt_state: NmtState,
        flags: PResFlags,
        pdo_version: PDOVersion,
        payload: Vec<u8>,
    ) -> Self {
        let eth_header = EthernetHeader::new(MacAddress(C_DLL_MULTICAST_PRES), source_mac);
        let payload_size = payload.len() as u16;

        PResFrame {
            eth_header,
            message_type: MessageType::PRes,
            destination: NodeId(C_ADR_BROADCAST_NODE_ID),
            source: source_node_id,
            nmt_state,
            flags,
            pdo_version,
            payload_size,
            payload,
        }
    }
}

impl Codec for PResFrame {
    fn serialize(&self, buffer: &mut [u8]) -> Result<usize, PowerlinkError> {
        let header_size = 24;
        let total_size = header_size + self.payload.len();
        if buffer.len() < total_size {
            return Err(PowerlinkError::BufferTooShort);
        }

        CodecHelpers::serialize_eth_header(&self.eth_header, buffer);
        CodecHelpers::serialize_pl_header(self.message_type, self.destination, self.source, buffer);

        buffer[17] = self.nmt_state as u8;
        let mut octet4 = 0u8;
        if self.flags.ms {
            octet4 |= 1 << 5;
        }
        if self.flags.en {
            octet4 |= 1 << 4;
        }
        if self.flags.rd {
            octet4 |= 1 << 0;
        }
        buffer[18] = octet4;
        let octet5 = (self.flags.pr as u8) << 3 | self.flags.rs.get();
        buffer[19] = octet5;
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
        let nmt_state = NmtState::try_from(buffer[17])?;

        let octet4 = buffer[18];
        let octet5 = buffer[19];

        let flags = PResFlags {
            ms: (octet4 & (1 << 5)) != 0,
            en: (octet4 & (1 << 4)) != 0,
            rd: (octet4 & (1 << 0)) != 0,
            // Safety: The 3 bits for PR are always a valid PRFlag variant.
            pr: unsafe { core::mem::transmute((octet5 >> 3) & 0b111) },
            rs: RSFlag::new(octet5 & 0b111),
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
            nmt_state,
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
    use crate::types::{C_DLL_MULTICAST_PRES, MessageType};
    use alloc::vec;

    #[test]
    fn test_presframe_new_constructor() {
        let source_mac = MacAddress([0xBB; 6]);
        let source_node = NodeId(10);
        let payload = vec![0xA, 0xB, 0xC, 0xD];
        let flags = PResFlags {
            ms: true,
            en: true,
            rd: false,
            pr: PRFlag::High1,
            rs: RSFlag::new(5),
        };
        let frame = PResFrame::new(
            source_mac,
            source_node,
            NmtState::NmtNotActive,
            flags,
            PDOVersion(1),
            payload.clone(),
        );

        assert_eq!(frame.eth_header.destination_mac.0, C_DLL_MULTICAST_PRES);
        assert_eq!(frame.message_type, MessageType::PRes);
        assert_eq!(frame.source, source_node);
        assert_eq!(frame.payload, payload);
        assert!(!frame.flags.rd);
        assert_eq!(frame.flags.rs.get(), 5);
    }

    #[test]
    fn test_pres_codec_roundtrip() {
        let original_frame = PResFrame::new(
            MacAddress([0xCC; 6]),
            NodeId(10),
            NmtState::NmtOperational,
            PResFlags {
                ms: false,
                en: true,
                rd: true,
                pr: PRFlag::PrioNmtRequest,
                rs: RSFlag::new(7),
            },
            PDOVersion(1),
            vec![0x11, 0x22, 0x33, 0x44, 0x55, 0x66],
        );

        let mut buffer = [0u8; 128];
        let bytes_written = original_frame.serialize(&mut buffer).unwrap();

        let deserialized_frame = PResFrame::deserialize(&buffer[..bytes_written]).unwrap();

        assert_eq!(original_frame, deserialized_frame);
    }

    #[test]
    fn test_pres_codec_empty_payload() {
        let original_frame = PResFrame::new(
            MacAddress([0xDD; 6]),
            NodeId(20),
            NmtState::NmtPreOperational2,
            PResFlags::default(),
            PDOVersion(0),
            vec![],
        );

        let mut buffer = [0u8; 128];
        let bytes_written = original_frame.serialize(&mut buffer).unwrap();
        assert_eq!(bytes_written, 60); // Padded to min ethernet size

        let deserialized_frame = PResFrame::deserialize(&buffer[..bytes_written]).unwrap();

        assert_eq!(original_frame, deserialized_frame);
        assert!(deserialized_frame.payload.is_empty());
    }

    #[test]
    fn test_pres_deserialize_short_buffer() {
        // Test short buffer for header
        let buffer = [0u8; 23];
        let result = PResFrame::deserialize(&buffer);
        assert!(matches!(result, Err(PowerlinkError::BufferTooShort)));

        // Test buffer that is too short for payload
        let original_frame = PResFrame::new(
            MacAddress([0xCC; 6]),
            NodeId(10),
            NmtState::NmtOperational,
            PResFlags {
                ms: false,
                en: true,
                rd: true,
                pr: PRFlag::PrioNmtRequest,
                rs: RSFlag::new(7),
            },
            PDOVersion(1),
            vec![0x11; 50], // Payload of 50 bytes
        );

        let mut long_buffer = [0u8; 100];
        original_frame.serialize(&mut long_buffer).unwrap();

        // Slice the buffer to be long enough for the header, but not the payload.
        let short_slice = &long_buffer[..40]; // Header=24, payload=50, total_len=74. Slice is 40.
        let result_payload = PResFrame::deserialize(short_slice);
        assert!(matches!(
            result_payload,
            Err(PowerlinkError::BufferTooShort)
        ));
    }
}
