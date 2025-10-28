// crates/powerlink-rs/src/frame/control/soa.rs

use crate::PowerlinkError;
use crate::frame::basic::{EthernetHeader, MacAddress};
use crate::frame::codec::{Codec, CodecHelpers};
use crate::nmt::states::NmtState;
use crate::types::{
    C_ADR_BROADCAST_NODE_ID, C_ADR_MN_DEF_NODE_ID, C_DLL_MULTICAST_SOA, EPLVersion, MessageType,
    NodeId,
};

/// Requested Service IDs for SoA frames.
/// (Reference: EPSG DS 301, Appendix 3.4)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RequestedServiceId {
    /// Corresponds to `NO_SERVICE`.
    NoService = 0x00,
    /// Corresponds to `IDENT_REQUEST`.
    IdentRequest = 0x01,
    /// Corresponds to `STATUS_REQUEST`.
    StatusRequest = 0x02,
    /// Corresponds to `NMT_REQUEST_INVITE`.
    NmtRequestInvite = 0x03,
    /// Corresponds to `UNSPECIFIED_INVITE`.
    UnspecifiedInvite = 0xFF,
}

impl TryFrom<u8> for RequestedServiceId {
    type Error = PowerlinkError;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x00 => Ok(Self::NoService),
            0x01 => Ok(Self::IdentRequest),
            0x02 => Ok(Self::StatusRequest),
            0x03 => Ok(Self::NmtRequestInvite),
            0xFF => Ok(Self::UnspecifiedInvite),
            // Updated error type to match definition
            _ => Err(PowerlinkError::InvalidRequestedServiceId(value)),
        }
    }
}

/// Represents a complete SoA frame.
/// (Reference: EPSG DS 301, Section 4.6.1.1.5)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SoAFrame {
    pub eth_header: EthernetHeader,
    pub message_type: MessageType,
    pub destination: NodeId,
    pub source: NodeId,
    pub nmt_state: NmtState,
    pub flags: SoAFlags,
    pub req_service_id: RequestedServiceId,
    pub target_node_id: NodeId,
    pub epl_version: EPLVersion,
}

/// Flags specific to the SoA frame.
/// (Reference: EPSG DS 301, Table 22)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SoAFlags {
    pub ea: bool, // Exception Acknowledge
    pub er: bool, // Exception Reset
}

impl SoAFrame {
    /// Creates a new SoA frame.
    pub fn new(
        source_mac: MacAddress,
        nmt_state: NmtState,
        flags: SoAFlags,
        requested_service: RequestedServiceId,
        target_node_id: NodeId,
        epl_version: EPLVersion,
    ) -> Self {
        let eth_header = EthernetHeader::new(MacAddress(C_DLL_MULTICAST_SOA), source_mac);

        SoAFrame {
            eth_header,
            message_type: MessageType::SoA,
            destination: NodeId(C_ADR_BROADCAST_NODE_ID),
            source: NodeId(C_ADR_MN_DEF_NODE_ID),
            nmt_state,
            flags,
            req_service_id: requested_service,
            target_node_id,
            epl_version,
        }
    }
}

impl Codec for SoAFrame {
    /// Serializes the SoA frame into the provided buffer.
    /// Assumes buffer starts *after* the Ethernet header.
    fn serialize(&self, buffer: &mut [u8]) -> Result<usize, PowerlinkError> {
        let pl_data_len = 9; // Data fields up to epl_version (offset 8)
        let min_eth_payload_after_header = 46; // Minimum Ethernet payload size after Eth header

        if buffer.len() < pl_data_len {
            return Err(PowerlinkError::BufferTooShort);
        }

        CodecHelpers::serialize_pl_header(self.message_type, self.destination, self.source, buffer);

        buffer[3] = self.nmt_state as u8;
        let mut octet4 = 0u8;
        if self.flags.ea {
            octet4 |= 1 << 2;
        }
        if self.flags.er {
            octet4 |= 1 << 1;
        }
        buffer[4] = octet4;
        buffer[5] = 0; // Reserved
        buffer[6] = self.req_service_id as u8;
        buffer[7] = self.target_node_id.0;
        buffer[8] = self.epl_version.0;

        // Per spec Table 21, data fields (incl. headers) up to octet 8 (pl_buffer[8])
        // And reserved from 9..45. Total PL frame section = 46 bytes.
        let pl_frame_len = pl_data_len.max(min_eth_payload_after_header); // Use derived length

        // Apply padding
        if buffer.len() < pl_frame_len {
            return Err(PowerlinkError::BufferTooShort);
        }
        buffer[pl_data_len..pl_frame_len].fill(0); // Pad with zeros

        Ok(pl_frame_len)
    }

    /// Deserializes an SoA frame from the provided buffer.
    /// Assumes buffer starts *after* the Ethernet header.
    fn deserialize(eth_header: EthernetHeader, buffer: &[u8]) -> Result<Self, PowerlinkError> {
        let pl_data_len = 9; // Minimum data length for SoA
        if buffer.len() < pl_data_len {
            return Err(PowerlinkError::BufferTooShort);
        }

        let (message_type, destination, source) = CodecHelpers::deserialize_pl_header(buffer)?;

        if message_type != MessageType::SoA {
            return Err(PowerlinkError::InvalidPlFrame);
        }

        let nmt_state = NmtState::try_from(buffer[3])?;
        let octet4 = buffer[4];
        let flags = SoAFlags {
            ea: (octet4 & (1 << 2)) != 0,
            er: (octet4 & (1 << 1)) != 0,
        };

        let req_service_id = RequestedServiceId::try_from(buffer[6])?;
        let target_node_id = NodeId(buffer[7]);
        let epl_version = EPLVersion(buffer[8]);

        Ok(Self {
            eth_header, // Use passed-in header
            message_type,
            destination,
            source,
            nmt_state,
            flags,
            req_service_id,
            target_node_id,
            epl_version,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::codec::CodecHelpers;
    use crate::types::C_DLL_MULTICAST_SOA; // Import for test setup

    #[test]
    fn test_soaframe_new_constructor() {
        let source_mac = MacAddress([0xFE, 0xDC, 0xBA, 0x98, 0x76, 0x54]);
        let target_node = NodeId(42);
        let service = RequestedServiceId::StatusRequest;
        let flags = SoAFlags {
            ea: true,
            er: false,
        };

        let frame = SoAFrame::new(
            source_mac,
            NmtState::NmtNotActive,
            flags,
            service,
            target_node,
            EPLVersion(1),
        );

        assert_eq!(frame.eth_header.destination_mac.0, C_DLL_MULTICAST_SOA);
        assert_eq!(frame.eth_header.source_mac, source_mac);
        assert_eq!(frame.message_type, MessageType::SoA);
        assert_eq!(frame.source, NodeId(C_ADR_MN_DEF_NODE_ID));
    }

    #[test]
    fn test_soa_codec_roundtrip() {
        let source_mac = MacAddress([0xFE, 0xDC, 0xBA, 0x98, 0x76, 0x54]);
        let original_frame = SoAFrame::new(
            source_mac,
            NmtState::NmtPreOperational1,
            SoAFlags {
                ea: true,
                er: false,
            },
            RequestedServiceId::StatusRequest,
            NodeId(42),
            EPLVersion(1),
        );

        let mut buffer = [0u8; 128]; // Full Ethernet frame buffer
        // 1. Serialize Eth header
        CodecHelpers::serialize_eth_header(&original_frame.eth_header, &mut buffer);
        // 2. Serialize PL frame part
        let pl_bytes_written = original_frame.serialize(&mut buffer[14..]).unwrap();

        // SoA PL frame section is always 46 bytes (padded)
        assert_eq!(pl_bytes_written, 46);
        let total_frame_len = 14 + pl_bytes_written;

        // 3. Deserialize full frame
        let deserialized_frame = crate::frame::deserialize_frame(&buffer[..total_frame_len])
            .unwrap()
            .into_soa() // Use helper
            .unwrap();

        assert_eq!(original_frame, deserialized_frame);
    }

    #[test]
    fn test_soa_deserialize_short_buffer() {
        // Test with buffer just short enough for the header but nothing else
        let eth_header = EthernetHeader::new(MacAddress([0; 6]), MacAddress([0; 6]));
        let short_buffer = [0u8; 8]; // Needs 9 bytes for PL part

        let result = SoAFrame::deserialize(eth_header, &short_buffer);
        // Line 245 in the original file corresponds to this assertion
        assert!(matches!(result, Err(PowerlinkError::BufferTooShort)));
    }
}
