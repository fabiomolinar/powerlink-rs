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
            _ => Err(PowerlinkError::InvalidServiceId(value)),
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
    fn serialize(&self, buffer: &mut [u8]) -> Result<usize, PowerlinkError> {
        const SOA_SIZE: usize = 60;
        if buffer.len() < SOA_SIZE {
            return Err(PowerlinkError::BufferTooShort);
        }

        CodecHelpers::serialize_eth_header(&self.eth_header, buffer);
        CodecHelpers::serialize_pl_header(self.message_type, self.destination, self.source, buffer);

        buffer[17] = self.nmt_state as u8;
        let mut octet4 = 0u8;
        if self.flags.ea {
            octet4 |= 1 << 2;
        }
        if self.flags.er {
            octet4 |= 1 << 1;
        }
        buffer[18] = octet4;
        buffer[19] = 0;
        buffer[20] = self.req_service_id as u8;
        buffer[21] = self.target_node_id.0;
        buffer[22] = self.epl_version.0;
        Ok(SOA_SIZE)
    }

    fn deserialize(buffer: &[u8]) -> Result<Self, PowerlinkError> {
        if buffer.len() < 60 {
            return Err(PowerlinkError::BufferTooShort);
        }

        let eth_header = CodecHelpers::deserialize_eth_header(buffer)?;
        let (message_type, destination, source) = CodecHelpers::deserialize_pl_header(buffer)?;
        let nmt_state = NmtState::try_from(buffer[17])?;

        let octet4 = buffer[18];
        let flags = SoAFlags {
            ea: (octet4 & (1 << 2)) != 0,
            er: (octet4 & (1 << 1)) != 0,
        };

        let req_service_id = RequestedServiceId::try_from(buffer[20])?;
        let target_node_id = NodeId(buffer[21]);
        let epl_version = EPLVersion(buffer[22]);

        Ok(Self {
            eth_header,
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
    use crate::types::C_DLL_MULTICAST_SOA;

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

        let mut buffer = [0u8; 128];
        let bytes_written = original_frame.serialize(&mut buffer).unwrap();
        assert!(bytes_written >= 60);

        let deserialized_frame = SoAFrame::deserialize(&buffer[..bytes_written]).unwrap();

        assert_eq!(original_frame, deserialized_frame);
    }

    #[test]
    fn test_soa_deserialize_short_buffer() {
        let buffer = [0u8; 59]; // One byte too short
        let result = SoAFrame::deserialize(&buffer);
        assert!(matches!(result, Err(PowerlinkError::BufferTooShort)));
    }
}
