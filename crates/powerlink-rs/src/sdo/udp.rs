// crates/powerlink-rs/src/sdo/udp.rs
//! Handles serialization and deserialization of SDO data within UDP payloads.
//! (Reference: EPSG DS 301, Section 6.3.2.1 and Table 47)

use crate::sdo::command::SdoCommand;
use crate::sdo::sequence::SequenceLayerHeader;
use crate::frame::ServiceId;
use crate::types::MessageType;
use crate::PowerlinkError;
use log::trace;

/// The fixed size prefix before the SDO Sequence Layer in a UDP payload.
/// MessageType(1) + Reserved(2) + ServiceID(1) = 4 bytes.
const UDP_SDO_PREFIX_SIZE: usize = 4;

/// Serializes an SDO Sequence Layer header and SDO Command into a UDP payload buffer.
///
/// The buffer should be appropriately sized (e.g., MTU).
/// Returns the total number of bytes written to the buffer.
///
/// UDP Payload Format:
/// - MessageType (1 byte, ASnd = 0x06)
/// - Reserved (2 bytes, 0x0000)
/// - ServiceID (1 byte, SDO = 0x05)
/// - Sequence Layer Header (4 bytes)
/// - Command Layer (Variable)
/// - SDO Payload Data (Variable)
pub fn serialize_sdo_udp_payload(
    seq_header: SequenceLayerHeader,
    cmd: SdoCommand,
    buffer: &mut [u8],
) -> Result<usize, PowerlinkError> {
    trace!(
        "Serializing SDO UDP payload: Seq={:?}, Cmd={:?}",
        seq_header, cmd
    );
    // 1. Check buffer size for the minimum required length (prefix + seq header)
    if buffer.len() < UDP_SDO_PREFIX_SIZE + 4 {
        return Err(PowerlinkError::BufferTooShort);
    }

    // 2. Write the POWERLINK UDP Prefix
    buffer[0] = MessageType::ASnd as u8; // MessageType = ASnd
    buffer[1..3].copy_from_slice(&[0u8, 0u8]); // Reserved bytes
    buffer[3] = ServiceId::Sdo as u8; // ServiceID = SDO

    // 3. Serialize the Sequence Layer Header
    let seq_len = seq_header.serialize(&mut buffer[UDP_SDO_PREFIX_SIZE..])?;
    let seq_end_offset = UDP_SDO_PREFIX_SIZE + seq_len;

    // 4. Serialize the SDO Command (Header + Payload)
    let cmd_len = cmd.serialize(&mut buffer[seq_end_offset..])?;
    let total_len = seq_end_offset + cmd_len;

    trace!("Serialized SDO UDP payload length: {}", total_len);
    Ok(total_len)
}

/// Deserializes an SDO Sequence Layer header and SDO Command from a received UDP payload buffer.
///
/// Returns the parsed SequenceLayerHeader and SdoCommand.
///
/// Assumes the buffer contains the *entire* UDP payload, starting with the POWERLINK UDP prefix.
pub fn deserialize_sdo_udp_payload(
    buffer: &[u8],
) -> Result<(SequenceLayerHeader, SdoCommand), PowerlinkError> {
    trace!(
        "Deserializing SDO UDP payload ({} bytes): {:02X?}",
        buffer.len(), buffer
    );
    // 1. Check minimum length (prefix + seq header) and prefix values
    if buffer.len() < UDP_SDO_PREFIX_SIZE + 4 {
        return Err(PowerlinkError::BufferTooShort);
    }
    if buffer[0] != MessageType::ASnd as u8 {
        return Err(PowerlinkError::InvalidPlFrame); // Use InvalidPlFrame as it's a framing error
    }
    // Ignore buffer[1..3] (Reserved)
    if buffer[3] != ServiceId::Sdo as u8 {
        return Err(PowerlinkError::InvalidServiceId(buffer[3]));
    }

    // 2. Deserialize Sequence Layer Header
    let seq_header = SequenceLayerHeader::deserialize(&buffer[UDP_SDO_PREFIX_SIZE..])?;
    let seq_end_offset = UDP_SDO_PREFIX_SIZE + 4; // Sequence header is fixed 4 bytes

    // 3. Deserialize SDO Command (Header + Payload) from the rest of the buffer
    let cmd = SdoCommand::deserialize(&buffer[seq_end_offset..])?;

    trace!(
        "Deserialized SDO UDP payload: Seq={:?}, Cmd={:?}",
        seq_header, cmd
    );
    Ok((seq_header, cmd))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sdo::command::{CommandId, CommandLayerHeader, Segmentation};
    use crate::sdo::sequence::{ReceiveConnState, SendConnState};
    use alloc::vec;

    #[test]
    fn test_sdo_udp_payload_roundtrip() {
        let original_seq = SequenceLayerHeader {
            receive_sequence_number: 10,
            receive_con: ReceiveConnState::ConnectionValid,
            send_sequence_number: 5,
            send_con: SendConnState::ConnectionValidAckRequest,
        };
        let original_cmd = SdoCommand {
            header: CommandLayerHeader {
                transaction_id: 1,
                is_response: false,
                is_aborted: false,
                segmentation: Segmentation::Expedited,
                command_id: CommandId::ReadByIndex,
                segment_size: 4, // Size of payload below
            },
            data_size: None,
            payload: vec![0x08, 0x10, 0x01, 0x00], // Read 0x1008/1
        };

        let mut buffer = [0u8; 100];
        let bytes_written =
            serialize_sdo_udp_payload(original_seq, original_cmd.clone(), &mut buffer).unwrap();

        // Expected Prefix: 0x06, 0x00, 0x00, 0x05
        assert_eq!(buffer[0], 0x06); // ASnd
        assert_eq!(&buffer[1..3], &[0x00, 0x00]); // Reserved
        assert_eq!(buffer[3], 0x05); // SDO

        // Prefix (4) + Seq (4) + Cmd Hdr (4) + Cmd Payload (4) = 16 bytes
        assert_eq!(bytes_written, 16);

        let (deserialized_seq, deserialized_cmd) =
            deserialize_sdo_udp_payload(&buffer[..bytes_written]).unwrap();

        assert_eq!(original_seq, deserialized_seq);
        assert_eq!(original_cmd, deserialized_cmd);
    }

    #[test]
    fn test_deserialize_sdo_udp_payload_errors() {
        // Buffer too short
        let short_buffer = [0x06, 0x00, 0x00, 0x05, 0xAA]; // Only 5 bytes
        assert!(matches!(
            deserialize_sdo_udp_payload(&short_buffer),
            Err(PowerlinkError::BufferTooShort)
        ));

        // Wrong MessageType
        let wrong_mtype = [
            0x01, 0x00, 0x00, 0x05, 0xAA, 0x3F, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        assert!(matches!(
            deserialize_sdo_udp_payload(&wrong_mtype),
            Err(PowerlinkError::InvalidPlFrame)
        ));

        // Wrong ServiceID
        let wrong_svcid = [
            0x06, 0x00, 0x00, 0x01, 0xAA, 0x3F, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        assert!(matches!(
            deserialize_sdo_udp_payload(&wrong_svcid),
            Err(PowerlinkError::InvalidServiceId(0x01))
        ));

        // Malformed Sequence Header (buffer ok, but invalid sequence state enum)
        let bad_seq = [
            0x06, 0x00, 0x00, 0x05, 0xFF, 0xFF, 0x00, 0x00, // Invalid rcon/scon values
            0x00, 0x00, 0x00, 0x00,
        ];
        assert!(matches!(
            deserialize_sdo_udp_payload(&bad_seq),
            Err(PowerlinkError::InvalidEnumValue) // Error from SequenceLayerHeader::deserialize
        ));

        // Malformed Command Header (buffer ok, but invalid command ID enum)
        let bad_cmd = [
            0x06, 0x00, 0x00, 0x05, 0xAA, 0x3F, 0x00, 0x00, // Valid prefix + seq
            0x01, 0xFF, 0x04, 0x00, // Invalid Cmd ID (0xFF)
            0x08, 0x10, 0x01, 0x00,
        ];
        assert!(matches!(
            deserialize_sdo_udp_payload(&bad_cmd),
            Err(PowerlinkError::InvalidEnumValue) // Error from SdoCommand::deserialize
        ));
    }
}
