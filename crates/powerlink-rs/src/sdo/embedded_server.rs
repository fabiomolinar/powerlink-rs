// crates/powerlink-rs/src/sdo/embedded_server.rs
//! Manages the server-side state for SDO transfers embedded in PDOs.
//!
//! (Reference: EPSG DS 301, Section 6.3.3)

use crate::PowerlinkError;
use crate::od::{ObjectDictionary, ObjectValue};
use crate::sdo::command::{CommandId, ReadByIndexRequest, Segmentation};
use crate::sdo::embedded::{PdoSdoCommand, PdoSequenceLayerHeader};
use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;
use log::{error, trace, warn};

/// The state of a single embedded SDO server connection.
#[derive(Debug, Clone, Default)]
struct EmbeddedSdoConnection {
    /// The last successfully processed sequence number (0-63).
    last_sequence_number: u8,
    /// The response payload that is ready to be sent.
    pending_response: Option<Vec<u8>>,
}

/// Manages all embedded SDO server channels (0x1200 - 0x127F).
#[derive(Debug, Default)]
pub struct EmbeddedSdoServer {
    /// Maps a channel index (e.g., 0x1200) to its connection state.
    connections: BTreeMap<u16, EmbeddedSdoConnection>,
}

impl EmbeddedSdoServer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Handles an incoming SDO request from an RPDO container.
    ///
    /// This function deserializes the request, processes it against the OD,
    /// and stores the generated response payload for the next TPDO.
    pub fn handle_request(
        &mut self,
        channel_index: u16,
        payload: &[u8],
        od: &mut ObjectDictionary, // Changed to mutable reference
    ) {
        let conn = self
            .connections
            .entry(channel_index)
            .or_insert_with(Default::default);

        // Deserialize the embedded command
        let Ok(command) = PdoSdoCommand::deserialize(payload) else {
            warn!(
                "[SDO-PDO] Server: Failed to deserialize request for channel {:#06X}",
                channel_index
            );
            return;
        };

        // Check sequence number (Spec 6.3.3.1.2.1, 6.3.3.1.2.2)
        let expected_seq = conn.last_sequence_number.wrapping_add(1) % 64;
        if command.sequence_header.sequence_number == conn.last_sequence_number {
            trace!(
                "[SDO-PDO] Server: Received duplicate request (Seq {}) for channel {:#06X}. Resending last response.",
                command.sequence_header.sequence_number, channel_index
            );
            // Don't re-process, just let get_pending_response send the same response again.
            return;
        } else if command.sequence_header.sequence_number != expected_seq {
            error!(
                "[SDO-PDO] Server: Sequence number mismatch for channel {:#06X}. Expected {}, got {}. Ignoring.",
                channel_index, expected_seq, command.sequence_header.sequence_number
            );
            // TODO: Per spec, we should send an Error Response (con=3).
            // For now, we ignore and don't update our sequence number.
            return;
        }

        // Sequence is new and valid, update our state.
        conn.last_sequence_number = command.sequence_header.sequence_number;

        // Process the SDO command
        let response_payload = Self::process_command(&command, od, conn.last_sequence_number);

        // Store the serialized response
        conn.pending_response = Some(response_payload);
    }

    /// Generates a response payload for a given request.
    fn process_command(
        req: &PdoSdoCommand,
        od: &mut ObjectDictionary,
        response_seq_num: u8,
    ) -> Vec<u8> {
        let (abort_code, data) = match req.command_id {
            CommandId::ReadByIndex => {
                // For Embedded SDO, Index and SubIndex are in the header fields (req.index, req.sub_index).
                // The payload (req.data) is typically empty for a Read request.
                match od.read(req.index, req.sub_index) {
                    Some(value) => (None, value.serialize()),
                    None => {
                        // Distinguish Object vs SubObject not found
                        if od.read_object(req.index).is_none() {
                            (Some(0x0602_0000), Vec::new()) // Object does not exist
                        } else {
                            (Some(0x0609_0011), Vec::new()) // Sub-index does not exist
                        }
                    }
                }
            }
            CommandId::WriteByIndex => {
                // 1. Check if object exists and get its type template
                // We clone the template to avoid holding an immutable borrow on OD while calling write (mutable borrow)
                let template_opt = od
                    .read(req.index, req.sub_index)
                    .map(|cow| cow.into_owned());

                if let Some(template) = template_opt {
                    // 2. Deserialize the raw data using the type template
                    match ObjectValue::deserialize(&req.data, &template) {
                        Ok(value) => {
                            // 3. Write the value to the OD
                            match od.write(req.index, req.sub_index, value) {
                                Ok(_) => (None, Vec::new()), // Success
                                Err(e) => {
                                    // Map PowerlinkError to SDO Abort Code
                                    let code = match e {
                                        PowerlinkError::StorageError("Object is read-only") => {
                                            0x0601_0002
                                        }
                                        PowerlinkError::TypeMismatch => 0x0607_0010,
                                        PowerlinkError::ValidationError(_) => 0x0609_0030, // Value range exceeded
                                        _ => 0x0800_0020, // Data cannot be transferred or stored
                                    };
                                    (Some(code), Vec::new())
                                }
                            }
                        }
                        Err(_) => (Some(0x0607_0010), Vec::new()), // Data type does not match (deserialization failed)
                    }
                } else {
                    // Object or sub-index does not exist
                    if od.read_object(req.index).is_none() {
                        (Some(0x0602_0000), Vec::new())
                    } else {
                        (Some(0x0609_0011), Vec::new())
                    }
                }
            }
            _ => (Some(0x0504_0001), Vec::new()), // Unsupported command / Command ID not valid
        };

        let response_cmd = PdoSdoCommand {
            sequence_header: PdoSequenceLayerHeader {
                sequence_number: response_seq_num,
                connection_state: if abort_code.is_some() { 3 } else { 2 }, // 3=Error, 2=Valid
            },
            transaction_id: req.transaction_id,
            is_response: true,
            is_aborted: abort_code.is_some(),
            segmentation: Segmentation::Expedited, // Always expedited for embedded
            valid_payload_length: 0,               // Will be set during serialization
            command_id: if abort_code.is_some() {
                CommandId::Nil
            } else {
                req.command_id
            },
            index: req.index,
            sub_index: req.sub_index,
            data: if let Some(code) = abort_code {
                (code as u32).to_le_bytes().to_vec()
            } else {
                data
            },
        };

        response_cmd.serialize()
    }

    /// Retrieves the pending response payload for a TPDO container.
    /// This consumes the pending response.
    pub fn get_pending_response(&mut self, channel_index: u16, container_len: usize) -> Vec<u8> {
        let conn = self
            .connections
            .entry(channel_index)
            .or_insert_with(Default::default);

        if let Some(payload) = conn.pending_response.take() {
            if payload.len() > container_len {
                error!(
                    "[SDO-PDO] Server: Response for {:#06X} ({} bytes) exceeds container length ({} bytes).",
                    channel_index,
                    payload.len(),
                    container_len
                );
                // Truncate the response
                payload[..container_len].to_vec()
            } else {
                // Pad the response to fill the container
                let mut padded_payload = payload;
                padded_payload.resize(container_len, 0);
                padded_payload
            }
        } else {
            // No response ready, send an "Idle/NIL" command
            vec![0; container_len]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::od::{AccessType, Object, ObjectEntry, ObjectValue};
    use crate::sdo::command::{CommandId, Segmentation};
    use crate::sdo::embedded::{PdoSdoCommand, PdoSequenceLayerHeader};
    use alloc::vec;

    fn create_test_od() -> ObjectDictionary<'static> {
        let mut od = ObjectDictionary::new(None);
        // Add a test object
        od.insert(
            0x2000,
            ObjectEntry {
                object: Object::Variable(ObjectValue::Unsigned32(0xDEADBEEF)),
                access: Some(AccessType::ReadWrite), // Make it writable
                ..Default::default()
            },
        );
        od
    }

    #[test]
    fn test_handle_valid_read_request() {
        let mut server = EmbeddedSdoServer::new();
        let mut od = create_test_od();

        let request = PdoSdoCommand {
            sequence_header: PdoSequenceLayerHeader {
                sequence_number: 1,
                connection_state: 2,
            },
            transaction_id: 10,
            is_response: false,
            is_aborted: false,
            segmentation: Segmentation::Expedited,
            valid_payload_length: 0, // Read request has no data payload
            command_id: CommandId::ReadByIndex,
            index: 0x2000,
            sub_index: 0,
            data: vec![], // Empty data for Read
        };

        // Serialize request
        let payload = request.serialize();

        // Handle it
        server.handle_request(0x1200, &payload, &mut od);

        // Retrieve response
        let response_bytes = server.get_pending_response(0x1200, 20);
        let response =
            PdoSdoCommand::deserialize(&response_bytes).expect("Failed to parse response");

        assert!(response.is_response);
        assert!(!response.is_aborted);
        assert_eq!(response.transaction_id, 10);
        assert_eq!(response.index, 0x2000);

        // Verify data: 0xDEADBEEF (LE: EF BE AD DE)
        assert_eq!(response.data, vec![0xEF, 0xBE, 0xAD, 0xDE]);
    }

    #[test]
    fn test_handle_valid_write_request() {
        let mut server = EmbeddedSdoServer::new();
        let mut od = create_test_od();

        let request = PdoSdoCommand {
            sequence_header: PdoSequenceLayerHeader {
                sequence_number: 1,
                connection_state: 2,
            },
            transaction_id: 15,
            is_response: false,
            is_aborted: false,
            segmentation: Segmentation::Expedited,
            valid_payload_length: 4,
            command_id: CommandId::WriteByIndex,
            index: 0x2000,
            sub_index: 0,
            // Write 0xCAFEBABE (LE: BE BA FE CA)
            data: vec![0xBE, 0xBA, 0xFE, 0xCA],
        };

        let payload = request.serialize();
        server.handle_request(0x1200, &payload, &mut od);

        // Retrieve response
        let response_bytes = server.get_pending_response(0x1200, 20);
        let response =
            PdoSdoCommand::deserialize(&response_bytes).expect("Failed to parse response");

        assert!(response.is_response);
        assert!(!response.is_aborted);
        assert_eq!(response.transaction_id, 15);

        // Verify write occurred in OD
        let val = od.read_u32(0x2000, 0).unwrap();
        assert_eq!(val, 0xCAFEBABE);
    }

    #[test]
    fn test_handle_duplicate_request() {
        let mut server = EmbeddedSdoServer::new();
        let mut od = create_test_od();

        let request = PdoSdoCommand {
            sequence_header: PdoSequenceLayerHeader {
                sequence_number: 1,
                connection_state: 2,
            },
            transaction_id: 5,
            is_response: false,
            is_aborted: false,
            segmentation: Segmentation::Expedited,
            valid_payload_length: 0,
            command_id: CommandId::ReadByIndex,
            index: 0x2000,
            sub_index: 0,
            data: vec![],
        };

        let payload = request.serialize();

        // 1. First request
        server.handle_request(0x1200, &payload, &mut od);
        let resp1 = server.get_pending_response(0x1200, 20);
        assert!(!resp1.iter().all(|&x| x == 0));

        // 2. Duplicate request (same seq)
        server.handle_request(0x1200, &payload, &mut od);
        let resp2 = server.get_pending_response(0x1200, 20);

        // Since pending_response was taken, it will be NIL (zeros).
        assert!(resp2.iter().all(|&x| x == 0));
    }

    #[test]
    fn test_handle_invalid_index() {
        let mut server = EmbeddedSdoServer::new();
        let mut od = create_test_od();

        let request = PdoSdoCommand {
            sequence_header: PdoSequenceLayerHeader {
                sequence_number: 1,
                connection_state: 2,
            },
            transaction_id: 11,
            is_response: false,
            is_aborted: false,
            segmentation: Segmentation::Expedited,
            valid_payload_length: 0,
            command_id: CommandId::ReadByIndex,
            index: 0x9999, // Invalid
            sub_index: 0,
            data: vec![],
        };

        server.handle_request(0x1200, &request.serialize(), &mut od);
        let response_bytes = server.get_pending_response(0x1200, 20);
        let response = PdoSdoCommand::deserialize(&response_bytes).unwrap();

        assert!(response.is_aborted);
        // Abort Code: Object does not exist (0x06020000) -> LE: [00, 00, 02, 06]
        assert_eq!(response.data, vec![0x00, 0x00, 0x02, 0x06]);
    }
}