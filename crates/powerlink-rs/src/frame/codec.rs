use crate::PowerlinkError;
use crate::frame::{
    ASndFrame, EthernetHeader, PReqFrame, PResFrame, PowerlinkFrame, SoAFrame, SocFrame,
    basic::MacAddress,
};
use crate::types::{MessageType, NodeId};
use log::debug;

/// A trait for objects that can be serialized into and deserialized from a byte buffer.
pub trait Codec: Sized {
    /// Serializes the object into the provided buffer.
    /// Assumes buffer starts *after* the Ethernet header (i.e., at the MessageType field).
    /// Returns the number of bytes written to the buffer for the POWERLINK section,
    /// including necessary padding for minimum Ethernet frame size.
    fn serialize(&self, buffer: &mut [u8]) -> Result<usize, PowerlinkError>;

    /// Deserializes an object from the provided buffer.
    /// Assumes the buffer starts *after* the Ethernet header.
    /// The eth_header is passed in separately, as it was parsed by the caller.
    fn deserialize(eth_header: EthernetHeader, buffer: &[u8]) -> Result<Self, PowerlinkError>;
}

/// Contains helper functions for common serialization/deserialization tasks.
pub struct CodecHelpers;
impl CodecHelpers {
    /// Serializes the 14-byte Ethernet header into the start of a buffer.
    pub fn serialize_eth_header(header: &EthernetHeader, buffer: &mut [u8]) {
        if buffer.len() >= 14 {
            buffer[0..6].copy_from_slice(&header.destination_mac.0);
            buffer[6..12].copy_from_slice(&header.source_mac.0);
            buffer[12..14].copy_from_slice(&header.ether_type.to_be_bytes());
        }
    }

    /// Serializes the common POWERLINK header fields (MessageType, Dest, Src).
    /// Assumes buffer starts *at* the POWERLINK frame section (after Eth header).
    pub fn serialize_pl_header(
        message_type: MessageType,
        destination: NodeId,
        source: NodeId,
        buffer: &mut [u8],
    ) {
        if buffer.len() >= 3 {
            buffer[0] = message_type as u8; // Message type at index 0
            buffer[1] = destination.0;    // Dest Node ID at index 1
            buffer[2] = source.0;         // Src Node ID at index 2
        }
    }

    /// Deserializes the 14-byte Ethernet header from the start of a buffer.
    /// Returns BufferTooShort if the buffer is too small for any header field.
    pub fn deserialize_eth_header(buffer: &[u8]) -> Result<EthernetHeader, PowerlinkError> {
        if buffer.len() < 14 {
            return Err(PowerlinkError::BufferTooShort);
        }
        // Map TryFromSliceError to BufferTooShort for robustness
        Ok(EthernetHeader {
            destination_mac: MacAddress(buffer[0..6].try_into().map_err(|_| PowerlinkError::BufferTooShort)?),
            source_mac: MacAddress(buffer[6..12].try_into().map_err(|_| PowerlinkError::BufferTooShort)?),
            ether_type: u16::from_be_bytes(buffer[12..14].try_into().map_err(|_| PowerlinkError::BufferTooShort)?),
        })
    }


    /// Deserializes the common POWERLINK header fields (MessageType, Dest, Src).
    /// Assumes buffer starts *at* the POWERLINK frame section (after Eth header).
    pub fn deserialize_pl_header(
        buffer: &[u8],
    ) -> Result<(MessageType, NodeId, NodeId), PowerlinkError> {
        if buffer.len() < 3 { // Need minimum 3 bytes for MType, Dest, Src
            return Err(PowerlinkError::BufferTooShort);
        }
        // Message type is in the lower 7 bits of the 1st byte (index 0)
        let message_type = MessageType::try_from(buffer[0] & 0x7F)?;
        let destination = NodeId(buffer[1]);
        let source = NodeId(buffer[2]);
        Ok((message_type, destination, source))
    }
}

/// Parses a raw byte buffer (including Ethernet header) and returns the corresponding `PowerlinkFrame`.
pub fn deserialize_frame(buffer: &[u8]) -> Result<PowerlinkFrame, PowerlinkError> {
    // 1. Deserialize Ethernet Header (checks for min length 14)
    let eth_header = CodecHelpers::deserialize_eth_header(buffer)?;

    // 2. Check EtherType
    if !eth_header.is_powerlink() {
        return Err(PowerlinkError::InvalidEthernetFrame);
    }

    // 3. Get POWERLINK frame section (everything after the Eth header)
    let pl_buffer = &buffer[14..];

    // 4. Check for minimum PL frame size (at least MessageType byte)
    //    CodecHelpers::deserialize_pl_header will check for len < 3 later if needed.
    if pl_buffer.is_empty() {
        return Err(PowerlinkError::InvalidPlFrame); // Specifically PL frame invalid
    }

    // 5. Get MessageType
    let message_type_byte = pl_buffer[0] & 0x7F;

    // 6. Call the appropriate frame-specific deserialize method
    let result = match MessageType::try_from(message_type_byte) {
        Ok(MessageType::SoC) => SocFrame::deserialize(eth_header, pl_buffer).map(PowerlinkFrame::Soc),
        Ok(MessageType::PReq) => PReqFrame::deserialize(eth_header, pl_buffer).map(PowerlinkFrame::PReq),
        Ok(MessageType::PRes) => PResFrame::deserialize(eth_header, pl_buffer).map(PowerlinkFrame::PRes),
        Ok(MessageType::SoA) => SoAFrame::deserialize(eth_header, pl_buffer).map(PowerlinkFrame::SoA),
        Ok(MessageType::ASnd) => ASndFrame::deserialize(eth_header, pl_buffer).map(PowerlinkFrame::ASnd),
        Err(_) => Err(PowerlinkError::InvalidMessageType(message_type_byte)),
    };

    if let Ok(frame) = &result {
        debug!("Successfully deserialized frame: {:?}", frame);
    }

    result
}
