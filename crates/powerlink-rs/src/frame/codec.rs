// In frame/codec.rs

use crate::frame::{
    ASndFrame, PowerlinkFrame, PReqFrame, PResFrame, SoAFrame, SocFrame, EthernetHeader,
    basic::MacAddress
};
use crate::types::{MessageType, NodeId};
use crate::PowerlinkError;
use log::debug;

/// A trait for objects that can be serialized into and deserialized from a byte buffer.
pub trait Codec: Sized {
    /// Serializes the object into the provided buffer.
    fn serialize(&self, buffer: &mut [u8]) -> Result<usize, PowerlinkError>;

    /// Deserializes an object from the provided buffer.
    fn deserialize(buffer: &[u8]) -> Result<Self, PowerlinkError>;
}

/// Contains helper functions for common serialization/deserialization tasks.
pub(super) struct CodecHelpers;
impl CodecHelpers {
    /// Serializes the 14-byte Ethernet header.
    pub(super) fn serialize_eth_header(header: &EthernetHeader, buffer: &mut [u8]) {
        buffer[0..6].copy_from_slice(&header.destination_mac.0);
        buffer[6..12].copy_from_slice(&header.source_mac.0);
        buffer[12..14].copy_from_slice(&header.ether_type.to_be_bytes());
    }

    /// Serializes the common POWERLINK header fields (MessageType, Dest, Src).
    pub(super) fn serialize_pl_header(
        message_type: MessageType,
        destination: NodeId,
        source: NodeId,
        buffer: &mut [u8],
    ) {
        buffer[14] = message_type as u8;
        buffer[15] = destination.0;
        buffer[16] = source.0;
    }

    /// Deserializes the 14-byte Ethernet header.
    pub(super) fn deserialize_eth_header(buffer: &[u8]) -> Result<EthernetHeader, PowerlinkError> {
        if buffer.len() < 14 { return Err(PowerlinkError::InvalidEthernetFrame); }
        Ok(EthernetHeader {
            destination_mac: MacAddress(buffer[0..6].try_into()?),
            source_mac: MacAddress(buffer[6..12].try_into()?),
            ether_type: u16::from_be_bytes(buffer[12..14].try_into()?),
        })
    }

    /// Deserializes the common POWERLINK header fields (MessageType, Dest, Src).
    pub(super) fn deserialize_pl_header(buffer: &[u8]) -> Result<(MessageType, NodeId, NodeId), PowerlinkError> {
        if buffer.len() < 17 { return Err(PowerlinkError::InvalidPlFrame); }
        let message_type = MessageType::try_from(buffer[14] & 0x7F)?;
        let destination = NodeId(buffer[15]);
        let source = NodeId(buffer[16]);
        Ok((message_type, destination, source))
    }
}


/// Parses a raw byte buffer and returns the corresponding `PowerlinkFrame` enum.
pub fn deserialize_frame(buffer: &[u8]) -> Result<PowerlinkFrame, PowerlinkError> {
    // A valid POWERLINK frame must have at least an Ethernet header (14 bytes)
    // and a message type field (1 byte).
    if buffer.len() < 15 {
        return Err(PowerlinkError::InvalidPlFrame);
    }
    
    // The message type is in the lower 7 bits of the 15th byte (index 14).
    let message_type_byte = buffer[14] & 0x7F;
    
    let result = match MessageType::try_from(message_type_byte) {
        Ok(MessageType::SoC) => SocFrame::deserialize(buffer).map(PowerlinkFrame::Soc),
        Ok(MessageType::PReq) => PReqFrame::deserialize(buffer).map(PowerlinkFrame::PReq),
        Ok(MessageType::PRes) => PResFrame::deserialize(buffer).map(PowerlinkFrame::PRes),
        Ok(MessageType::SoA) => SoAFrame::deserialize(buffer).map(PowerlinkFrame::SoA),
        Ok(MessageType::ASnd) => ASndFrame::deserialize(buffer).map(PowerlinkFrame::ASnd),
        Err(_) => Err(PowerlinkError::InvalidPlFrame),
    };

    if let Ok(frame) = &result {
        debug!("Successfully deserialized frame: {:?}", frame);
    }
    
    result
}
