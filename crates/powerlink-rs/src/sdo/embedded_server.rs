// crates/powerlink-rs/src/sdo/embedded_server.rs
//! Manages the server-side state for SDO transfers embedded in PDOs.
//!
//! (Reference: EPSG DS 301, Section 6.3.3)

use crate::od::ObjectDictionary;
use crate::sdo::command::{CommandId, ReadByIndexRequest, Segmentation};
use crate::sdo::embedded::{PdoSdoCommand, PdoSequenceLayerHeader};
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use log::{error, trace, warn}; // Added warn

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
        od: &ObjectDictionary, // <-- ADDED
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
                command.sequence_header.sequence_number,
                channel_index
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

        // Process the SDO command (this is a simplified handler)
        let response_payload =
            Self::process_command(&command, od, conn.last_sequence_number);

        // Store the serialized response
        conn.pending_response = Some(response_payload);
    }

    /// Generates a response payload for a given request.
    /// This is a simplified handler for ReadByIndex only.
    fn process_command(
        req: &PdoSdoCommand,
        od: &ObjectDictionary,
        response_seq_num: u8,
    ) -> Vec<u8> {
        let (abort_code, data) = match req.command_id {
            CommandId::ReadByIndex => {
                match ReadByIndexRequest::from_payload(&req.data) {
                    Ok(read_req) => {
                        match od.read(read_req.index, read_req.sub_index) {
                            Some(value) => (None, value.serialize()),
                            None => (Some(0x0602_0000), Vec::new()), // Object does not exist
                        }
                    }
                    Err(_) => (Some(0x0504_0001), Vec::new()), // Invalid command
                }
            }
            _ => (Some(0x0504_0001), Vec::new()), // Unsupported command
        };

        let response_cmd = PdoSdoCommand {
            sequence_header: PdoSequenceLayerHeader {
                sequence_number: response_seq_num,
                connection_state: if abort_code.is_some() { 3 } else { 2 }, // 3=Error, 2=Valid
            },
            transaction_id: req.transaction_id,
            is_response: true,
            is_aborted: abort_code.is_some(),
            segmentation: Segmentation::Expedited, // Always expedited
            valid_payload_length: 0, // Will be set during serialization
            command_id: if abort_code.is_some() {
                CommandId::Nil
            } else {
                req.command_id
            },
            index: req.index,
            sub_index: req.sub_index,
            data: if let Some(code) = abort_code {
                // FIX: Explicitly type `code` as u32
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
                // Truncate the response (undesirable, but better than panic)
                payload[..container_len].to_vec()
            } else {
                // Pad the response to fill the container
                let mut padded_payload = payload;
                padded_payload.resize(container_len, 0);
                padded_payload
            }
        } else {
            // No response ready, send an "Idle/NIL" command
            // TODO: Send a proper NIL command instead of just zeros.
            vec![0; container_len]
        }
    }
}