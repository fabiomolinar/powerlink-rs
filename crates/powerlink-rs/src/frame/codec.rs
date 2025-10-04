// In frame/codec.rs

use crate::frame::{
    ASndFrame, PowerlinkFrame, PReqFrame, PResFrame, SoAFrame, SocFrame
};
use crate::types::MessageType;
use crate::PowerlinkError;

/// A trait for objects that can be serialized into and deserialized from a byte buffer.
pub trait Codec: Sized {
    /// Serializes the object into the provided buffer.
    ///
    /// Returns the number of bytes written.
    fn serialize(&self, buffer: &mut [u8]) -> Result<usize, PowerlinkError>;

    /// Deserializes an object from the provided buffer.
    fn deserialize(buffer: &[u8]) -> Result<Self, PowerlinkError>;
}

/// Parses a raw byte buffer and returns the corresponding `PowerlinkFrame` enum.
///
/// This function acts as the primary entry point for deserialization. It inspects
/// the `MessageType` field and delegates to the appropriate frame-specific
/// `deserialize` implementation.
pub fn deserialize_frame(buffer: &[u8]) -> Result<PowerlinkFrame, PowerlinkError> {
    // A valid POWERLINK frame must have at least an Ethernet header (14 bytes)
    // and a message type field (1 byte).
    if buffer.len() < 15 {
        return Err(PowerlinkError::InvalidFrame);
    }

    // The message type is in the lower 7 bits of the 15th byte (index 14).
    let message_type_byte = buffer[14] & 0x7F;

    match MessageType::try_from(message_type_byte) {
        Ok(MessageType::SoC) => SocFrame::deserialize(buffer).map(PowerlinkFrame::Soc),
        Ok(MessageType::PReq) => PReqFrame::deserialize(buffer).map(PowerlinkFrame::PReq),
        Ok(MessageType::PRes) => PResFrame::deserialize(buffer).map(PowerlinkFrame::PRes),
        Ok(MessageType::SoA) => SoAFrame::deserialize(buffer).map(PowerlinkFrame::SoA),
        Ok(MessageType::ASnd) => ASndFrame::deserialize(buffer).map(PowerlinkFrame::ASnd),
        Err(_) => Err(PowerlinkError::InvalidFrame),
    }
}