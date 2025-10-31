// crates/powerlink-rs/src/sdo/asnd.rs
//! Handles serialization of SDO data for ASnd frames.

use crate::PowerlinkError;
use crate::sdo::command::SdoCommand;
use crate::sdo::sequence::SequenceLayerHeader;
use alloc::vec;
use alloc::vec::Vec;

/// Helper to serialize SDO Sequence + Command into a buffer suitable for ASnd payload.
pub fn serialize_sdo_asnd_payload(
    seq_header: SequenceLayerHeader,
    cmd: SdoCommand,
) -> Result<Vec<u8>, PowerlinkError> {
    // Allocate buffer based on command payload size + headers
    let estimated_size = 4 // Sequence Header
                       + 4 // Command Header Fixed Part
                       + if cmd.data_size.is_some() { 4 } else { 0 } // Optional Data Size
                       + cmd.payload.len();
    // Use Vec directly instead of pre-allocating large buffer
    let mut buffer = Vec::with_capacity(estimated_size);

    // Serialize Sequence Layer Header (4 bytes)
    let mut seq_buf = [0u8; 4];
    seq_header.serialize(&mut seq_buf)?;
    buffer.extend_from_slice(&seq_buf);

    // Serialize Command Layer (Header + Payload)
    // Need a temporary buffer for cmd.serialize as it writes into a slice
    let mut cmd_buf = vec![0u8; estimated_size - 4]; // Max possible size for cmd part
    let cmd_len = cmd.serialize(&mut cmd_buf)?;
    buffer.extend_from_slice(&cmd_buf[..cmd_len]); // Append only the bytes written

    Ok(buffer)
}
