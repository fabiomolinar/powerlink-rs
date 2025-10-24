// crates/powerlink-rs/src/frame/control/soc.rs

use crate::PowerlinkError;
use crate::common::{NetTime, RelativeTime};
use crate::frame::basic::{EthernetHeader, MacAddress};
use crate::frame::codec::{Codec, CodecHelpers};
use crate::types::{
    C_ADR_BROADCAST_NODE_ID, C_ADR_MN_DEF_NODE_ID, C_DLL_MULTICAST_SOC, MessageType, NodeId,
};

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
    /// Serializes the SoC frame into the provided buffer.
    /// Assumes buffer starts *after* the Ethernet header.
    fn serialize(&self, buffer: &mut [u8]) -> Result<usize, PowerlinkError> {
        // Data fields up to relative_time end (index 21)
        let pl_data_len = 22;
        let min_eth_payload_after_header = 46; // Minimum Ethernet payload size after Eth header

        if buffer.len() < pl_data_len {
            return Err(PowerlinkError::BufferTooShort);
        }

        CodecHelpers::serialize_pl_header(self.message_type, self.destination, self.source, buffer);

        buffer[3] = 0; // Reserved
        let mut octet4 = 0u8;
        if self.flags.mc {
            octet4 |= 1 << 7;
        }
        if self.flags.ps {
            octet4 |= 1 << 6;
        }
        buffer[4] = octet4;
        buffer[5] = 0; // Reserved
        // NetTime starts at offset 6 (relative to PL frame)
        buffer[6..10].copy_from_slice(&self.net_time.seconds.to_le_bytes());
        buffer[10..14].copy_from_slice(&self.net_time.nanoseconds.to_le_bytes());
        // RelativeTime starts at offset 14
        buffer[14..18].copy_from_slice(&self.relative_time.seconds.to_le_bytes());
        buffer[18..22].copy_from_slice(&self.relative_time.nanoseconds.to_le_bytes());

        // Per spec Table 15, data fields (incl. headers) up to octet 21 (pl_buffer[21])
        // And reserved from 22..45. Total PL frame section = 46 bytes.
        let pl_frame_len = pl_data_len.max(min_eth_payload_after_header); // Use derived length

        // Apply padding
        if buffer.len() < pl_frame_len {
             return Err(PowerlinkError::BufferTooShort);
        }
        buffer[pl_data_len..pl_frame_len].fill(0); // Pad with zeros

        Ok(pl_frame_len)
    }

    /// Deserializes an SoC frame from the provided buffer.
    /// Assumes buffer starts *after* the Ethernet header.
    fn deserialize(eth_header: EthernetHeader, buffer: &[u8]) -> Result<Self, PowerlinkError> {
        // Minimum data length for SoC fields (up to end of relative_time)
        let pl_data_len = 22;
        if buffer.len() < pl_data_len {
            return Err(PowerlinkError::BufferTooShort);
        }

        let (message_type, destination, source) = CodecHelpers::deserialize_pl_header(buffer)?;

        if message_type != MessageType::SoC {
            return Err(PowerlinkError::InvalidPlFrame);
        }

        let octet4 = buffer[4];
        let flags = SocFlags {
            mc: (octet4 & (1 << 7)) != 0,
            ps: (octet4 & (1 << 6)) != 0,
        };

        // NetTime starts at offset 6
        // Map TryFromSliceError to BufferTooShort
        let net_time = NetTime {
            seconds: u32::from_le_bytes(buffer[6..10].try_into().map_err(|_| PowerlinkError::BufferTooShort)?),
            nanoseconds: u32::from_le_bytes(buffer[10..14].try_into().map_err(|_| PowerlinkError::BufferTooShort)?),
        };

        // RelativeTime starts at offset 14
        // Map TryFromSliceError to BufferTooShort
        let relative_time = RelativeTime {
            seconds: u32::from_le_bytes(buffer[14..18].try_into().map_err(|_| PowerlinkError::BufferTooShort)?),
            nanoseconds: u32::from_le_bytes(buffer[18..22].try_into().map_err(|_| PowerlinkError::BufferTooShort)?),
        };

        Ok(Self {
            eth_header, // Use the passed-in header
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
    use crate::frame::codec::CodecHelpers; // Import for test setup

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
        let flags = SocFlags {
            mc: true,
            ps: false,
        };
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
        let flags = SocFlags {
            mc: true,
            ps: false,
        };
        let original_frame = SocFrame::new(source_mac, flags, net_time, relative_time);

        let mut buffer = [0u8; 128]; // Full Ethernet frame buffer
        // 1. Serialize Eth header
        CodecHelpers::serialize_eth_header(&original_frame.eth_header, &mut buffer);
        // 2. Serialize PL frame part
        let pl_bytes_written = original_frame.serialize(&mut buffer[14..]).unwrap();

        // SoC PL frame section is always 46 bytes (padded)
        assert_eq!(pl_bytes_written, 46);
        let total_frame_len = 14 + pl_bytes_written;

        // 3. Deserialize full frame
        // Use the new helper method on PowerlinkFrame
        let deserialized_frame = crate::frame::deserialize_frame(&buffer[..total_frame_len])
            .unwrap()
            .into_soc() // Use helper
            .unwrap();

        assert_eq!(original_frame, deserialized_frame);
    }

    #[test]
    fn test_soc_deserialize_short_buffer() {
        // Test buffer just short enough to fail the last try_into() for relative_time.nanoseconds
        let eth_header = EthernetHeader::new(MacAddress([0; 6]), MacAddress([0; 6]));
        let short_buffer = [0u8; 21]; // Needs 22 bytes for PL part

        let result = SocFrame::deserialize(eth_header, &short_buffer);
        // Line 221 in the original file corresponds to this assertion
        assert!(matches!(result, Err(PowerlinkError::BufferTooShort)));
    }
}
