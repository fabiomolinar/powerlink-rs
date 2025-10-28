// crates/powerlink-rs/src/frame/poll/pres.rs
use crate::PowerlinkError;
use crate::frame::basic::{EthernetHeader, MacAddress};
use crate::frame::codec::{Codec, CodecHelpers}; // Added CodecHelpers
use crate::nmt::states::NmtState;
use crate::pdo::PDOVersion;
use crate::types::{C_ADR_BROADCAST_NODE_ID, C_DLL_MULTICAST_PRES, MessageType, NodeId};
use alloc::vec::Vec;
use log::warn; // Added warn for logging

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum PRFlag {
    /// Corresponds to `PRIO_NMT_REQUEST`.
    PrioNmtRequest = 0b111,
    High3 = 0b110,
    High2 = 0b101,
    High1 = 0b100,
    /// Corresponds to `PRIO_GENERIC_REQUEST`.
    #[default]
    PrioGenericRequest = 0b011,
    Low3 = 0b010,
    Low2 = 0b001,
    Low1 = 0b000,
}

impl TryFrom<u8> for PRFlag {
    type Error = PowerlinkError;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value & 0b111 {
            // Mask to get only the lower 3 bits
            0b111 => Ok(PRFlag::PrioNmtRequest),
            0b110 => Ok(PRFlag::High3),
            0b101 => Ok(PRFlag::High2),
            0b100 => Ok(PRFlag::High1),
            0b011 => Ok(PRFlag::PrioGenericRequest),
            0b010 => Ok(PRFlag::Low3),
            0b001 => Ok(PRFlag::Low2),
            0b000 => Ok(PRFlag::Low1),
            // Masking ensures this is unreachable, but added for completeness
            _ => Err(PowerlinkError::InvalidEnumValue),
        }
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
    /// Size of the actual payload data in bytes.
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
    /// Serializes the PRes frame into the provided buffer.
    /// Returns the total size of the POWERLINK frame section written,
    /// including padding if necessary to meet minimum Ethernet payload size.
    /// Assumes buffer starts *after* the Ethernet header.
    fn serialize(&self, buffer: &mut [u8]) -> Result<usize, PowerlinkError> {
        let pl_header_size = 10; // MType(1)+Dest(1)+Src(1)+NMTState(1)+Flags1(1)+Flags2(1)+PDOv(1)+Rsvd(1)+Size(2)
        let total_pl_frame_size = pl_header_size + self.payload.len();
        if buffer.len() < total_pl_frame_size {
            return Err(PowerlinkError::BufferTooShort);
        }

        // --- Serialize POWERLINK Header ---
        CodecHelpers::serialize_pl_header(self.message_type, self.destination, self.source, buffer);
        buffer[3] = self.nmt_state as u8;
        let mut octet4_flags1 = 0u8;
        if self.flags.ms {
            octet4_flags1 |= 1 << 5;
        }
        if self.flags.en {
            octet4_flags1 |= 1 << 4;
        }
        if self.flags.rd {
            octet4_flags1 |= 1 << 0;
        }
        buffer[4] = octet4_flags1;
        let octet5_flags2 = (self.flags.pr as u8) << 3 | self.flags.rs.get();
        buffer[5] = octet5_flags2;
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

    /// Deserializes a PRes frame from the provided buffer.
    /// Assumes the buffer starts *after* the 14-byte Ethernet header.
    fn deserialize(eth_header: EthernetHeader, buffer: &[u8]) -> Result<Self, PowerlinkError> {
        let pl_header_size = 10;
        if buffer.len() < pl_header_size {
            // Need at least the header
            return Err(PowerlinkError::BufferTooShort);
        }

        // Deserialize Basic PL Header
        let (message_type, destination, source) = CodecHelpers::deserialize_pl_header(buffer)?;

        // Validate message type
        if message_type != MessageType::PRes {
            return Err(PowerlinkError::InvalidPlFrame);
        }
        // Validate destination
        if destination.0 != C_ADR_BROADCAST_NODE_ID {
            // PRes must be broadcast
            warn!(
                "Received PRes frame with non-broadcast destination ID: {}",
                destination.0
            );
            return Err(PowerlinkError::InvalidPlFrame);
        }

        // Deserialize PRes Specific Header Fields
        let nmt_state = NmtState::try_from(buffer[3])?;
        let octet4_flags1 = buffer[4];
        let octet5_flags2 = buffer[5];
        let flags = PResFlags {
            ms: (octet4_flags1 & (1 << 5)) != 0,
            en: (octet4_flags1 & (1 << 4)) != 0,
            rd: (octet4_flags1 & (1 << 0)) != 0,
            pr: PRFlag::try_from(octet5_flags2 >> 3)?, // Use TryFrom
            rs: RSFlag::new(octet5_flags2 & 0b111),
        };
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
        let payload = buffer[payload_start..payload_end].to_vec();

        Ok(Self {
            eth_header, // Use the passed-in Eth header
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
    use crate::frame::codec::CodecHelpers;
    use crate::types::{C_DLL_MULTICAST_PRES, MessageType};
    use alloc::vec; // Needed for tests

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
            NmtState::NmtNotActive, // Example state
            flags,
            PDOVersion(1),
            payload.clone(),
        );

        assert_eq!(frame.eth_header.destination_mac.0, C_DLL_MULTICAST_PRES);
        assert_eq!(frame.eth_header.source_mac, source_mac); // Check source MAC
        assert_eq!(frame.message_type, MessageType::PRes);
        assert_eq!(frame.source, source_node);
        assert_eq!(frame.destination, NodeId(C_ADR_BROADCAST_NODE_ID)); // Check dest Node ID
        assert_eq!(frame.payload, payload);
        assert!(!frame.flags.rd);
        assert_eq!(frame.flags.rs.get(), 5);
        assert_eq!(frame.flags.pr, PRFlag::High1);
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
            vec![0x11, 0x22, 0x33, 0x44, 0x55, 0x66], // Payload len = 6
        );

        let mut buffer = [0u8; 128]; // Buffer for full Ethernet frame
        CodecHelpers::serialize_eth_header(&original_frame.eth_header, &mut buffer);
        let pl_bytes_written = original_frame.serialize(&mut buffer[14..]).unwrap();
        let total_frame_len = 14 + pl_bytes_written;

        // PL Header = 10, Payload = 6. Total = 16. < 46. Padded PL len = 46.
        assert_eq!(pl_bytes_written, 46);

        // Deserialize full frame
        let deserialized_frame = crate::frame::deserialize_frame(&buffer[..total_frame_len])
            .unwrap()
            .into_pres()
            .unwrap();

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
            vec![], // Payload len = 0
        );

        let mut buffer = [0u8; 128];
        CodecHelpers::serialize_eth_header(&original_frame.eth_header, &mut buffer);
        let pl_bytes_written = original_frame.serialize(&mut buffer[14..]).unwrap();
        let total_frame_len = 14 + pl_bytes_written;

        assert_eq!(pl_bytes_written, 46); // Padded to min ethernet payload size

        let deserialized_frame = crate::frame::deserialize_frame(&buffer[..total_frame_len])
            .unwrap()
            .into_pres()
            .unwrap();

        assert_eq!(original_frame, deserialized_frame);
        assert!(deserialized_frame.payload.is_empty());
        assert_eq!(deserialized_frame.payload_size, 0);
    }

    #[test]
    fn test_pres_deserialize_short_buffer() {
        // Test short buffer for header (less than 14 bytes for Eth header)
        let buffer_short_header = [0u8; 13];
        let result_header = crate::frame::deserialize_frame(&buffer_short_header);
        // Correct the assertion: Expect BufferTooShort, not InvalidEthernetFrame
        assert!(matches!(
            result_header,
            Err(PowerlinkError::BufferTooShort) // <-- FIX: Changed expected error
        ));

        // Test buffer that is too short for payload size indicated IN the frame
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
        CodecHelpers::serialize_eth_header(&original_frame.eth_header, &mut long_buffer);
        original_frame.serialize(&mut long_buffer[14..]).unwrap();
        // PL Header=10, Payload=50. Total=60. > 46. No padding.
        // Total bytes in buffer = 14 + 60 = 74.

        // Slice the buffer to be long enough for the header, but not the indicated payload.
        // Header = 14 (Eth) + 10 (PL) = 24. Indicated payload size = 50. Payload end = 14 + 10 + 50 = 74.
        let short_slice = &long_buffer[..40]; // Slice is 40 bytes long. Header fits, payload doesn't.
        let result_payload = crate::frame::deserialize_frame(short_slice);
        // deserialize_frame passes &short_slice[14..] (len 26) to PResFrame::deserialize
        // PResFrame::deserialize reads payload_size=50 from bytes 8-9 (indices 22-23 of full slice)
        // It checks if 26 < 10 + 50, which is true, and returns BufferTooShort.
        assert!(matches!(
            result_payload,
            Err(PowerlinkError::BufferTooShort)
        ));
    }
}
