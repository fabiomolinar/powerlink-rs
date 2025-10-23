// crates/powerlink-rs/src/frame/control/soc.rs

use crate::common::{NetTime, RelativeTime};
use crate::frame::basic::{EthernetHeader, MacAddress};
use crate::frame::codec::{Codec, CodecHelpers};
use crate::types::{
    MessageType, NodeId, C_ADR_BROADCAST_NODE_ID, C_ADR_MN_DEF_NODE_ID, C_DLL_MULTICAST_SOC,
};
use crate::PowerlinkError;

/// Represents a complete SoC frame.
/// (Reference: EPSG DS 301, Section 4.6.1.1.2)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SocFrame {
    pub eth_header: EthernetHeader,
    pub message_type: MessageType,
    pub destination: NodeId,
    pub source: NodeId,
    pub flags: SocFlags,
    pub net_time: NetTime,
    pub relative_time: RelativeTime,
}

/// Flags specific to the SoC frame.
/// (Reference: EPSG DS 301, Table 16)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SocFlags {
    pub mc: bool, // Multiplexed Cycle Completed
    pub ps: bool, // Prescaled Slot
}

impl SocFrame {
    /// Creates a new SoC frame.
    pub fn new(
        source_mac: MacAddress,
        flags: SocFlags,
        net_time: NetTime,
        relative_time: RelativeTime,
    ) -> Self {
        let eth_header = EthernetHeader::new(MacAddress(C_DLL_MULTICAST_SOC), source_mac);

        SocFrame {
            eth_header,
            message_type: MessageType::SoC,
            destination: NodeId(C_ADR_BROADCAST_NODE_ID),
            source: NodeId(C_ADR_MN_DEF_NODE_ID),
            flags,
            net_time,
            relative_time,
        }
    }
}

impl Codec for SocFrame {
    fn serialize(&self, buffer: &mut [u8]) -> Result<usize, PowerlinkError> {
        const SOC_SIZE: usize = 60;
        if buffer.len() < SOC_SIZE {
            return Err(PowerlinkError::BufferTooShort);
        }

        CodecHelpers::serialize_eth_header(&self.eth_header, buffer);
        CodecHelpers::serialize_pl_header(self.message_type, self.destination, self.source, buffer);

        buffer[17] = 0;
        let mut octet4 = 0u8;
        if self.flags.mc {
            octet4 |= 1 << 7;
        }
        if self.flags.ps {
            octet4 |= 1 << 6;
        }
        buffer[18] = octet4;
        buffer[19] = 0;
        buffer[20..24].copy_from_slice(&self.net_time.seconds.to_le_bytes());
        buffer[24..28].copy_from_slice(&self.net_time.nanoseconds.to_le_bytes());
        buffer[28..32].copy_from_slice(&self.relative_time.seconds.to_le_bytes());
        buffer[32..36].copy_from_slice(&self.relative_time.nanoseconds.to_le_bytes());

        Ok(SOC_SIZE)
    }

    fn deserialize(buffer: &[u8]) -> Result<Self, PowerlinkError> {
        if buffer.len() < 60 {
            return Err(PowerlinkError::BufferTooShort);
        }

        let eth_header = CodecHelpers::deserialize_eth_header(buffer)?;
        let (message_type, destination, source) = CodecHelpers::deserialize_pl_header(buffer)?;

        let octet4 = buffer[18];
        let flags = SocFlags {
            mc: (octet4 & (1 << 7)) != 0,
            ps: (octet4 & (1 << 6)) != 0,
        };

        let net_time = NetTime {
            seconds: u32::from_le_bytes(buffer[20..24].try_into()?),
            nanoseconds: u32::from_le_bytes(buffer[24..28].try_into()?),
        };

        let relative_time = RelativeTime {
            seconds: u32::from_le_bytes(buffer[28..32].try_into()?),
            nanoseconds: u32::from_le_bytes(buffer[32..36].try_into()?),
        };

        Ok(Self {
            eth_header,
            message_type,
            destination,
            source,
            flags,
            net_time,
            relative_time,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::C_DLL_MULTICAST_SOC;

    #[test]
    fn test_socframe_new_constructor() {
        let source_mac = MacAddress([0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC]);
        let dummy_time = NetTime {
            seconds: 0xABCD,
            nanoseconds: 0xABCD,
        };
        let dummy_rel_time = RelativeTime {
            seconds: 0xABCD,
            nanoseconds: 0xABCD,
        };
        let flags = SocFlags { mc: true, ps: false };
        let frame = SocFrame::new(source_mac, flags, dummy_time, dummy_rel_time);

        assert_eq!(frame.eth_header.destination_mac.0, C_DLL_MULTICAST_SOC);
        assert_eq!(frame.eth_header.source_mac, source_mac);
        assert_eq!(frame.message_type, MessageType::SoC);
        assert_eq!(frame.source, NodeId(C_ADR_MN_DEF_NODE_ID));
        assert_eq!(frame.destination, NodeId(C_ADR_BROADCAST_NODE_ID));
        assert_eq!(frame.flags.mc, true);
        assert_eq!(frame.flags.ps, false);
    }

    #[test]
    fn test_soc_codec_roundtrip() {
        let source_mac = MacAddress([0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC]);
        let net_time = NetTime {
            seconds: 123,
            nanoseconds: 456,
        };
        let relative_time = RelativeTime {
            seconds: 789,
            nanoseconds: 101,
        };
        let flags = SocFlags { mc: true, ps: false };
        let original_frame = SocFrame::new(source_mac, flags, net_time, relative_time);

        let mut buffer = [0u8; 128];
        let bytes_written = original_frame.serialize(&mut buffer).unwrap();

        // Ensure some bytes were written and it's at least the minimum frame size.
        assert!(bytes_written >= 60);

        let deserialized_frame = SocFrame::deserialize(&buffer[..bytes_written]).unwrap();

        assert_eq!(original_frame, deserialized_frame);
    }

    #[test]
    fn test_soc_deserialize_short_buffer() {
        let buffer = [0u8; 59]; // One byte too short
        let result = SocFrame::deserialize(&buffer);
        assert!(matches!(result, Err(PowerlinkError::BufferTooShort)));
    }
}