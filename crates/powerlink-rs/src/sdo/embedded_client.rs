// crates/powerlink-rs/src/sdo/embedded_client.rs
//! Manages the client-side state for SDO transfers embedded in PDOs.
//!
//! (Reference: EPSG DS 301, Section 6.3.3)
use crate::PowerlinkError;
use crate::sdo::command::{CommandId, Segmentation};
use crate::sdo::embedded::{PdoSdoCommand, PdoSequenceLayerHeader};
use alloc::collections::{BTreeMap, VecDeque};
use alloc::vec;
use alloc::vec::Vec;
use log::{error, info, trace, warn};

/// A request queued by the application to be sent via an embedded SDO client channel.
#[derive(Debug)] // <-- ADDED
struct SdoClientRequest {
    command_id: CommandId,
    index: u16,
    sub_index: u8,
    data: Vec<u8>,
    // TODO: Add a callback/waker for when the response is received
}

/// The state of a single embedded SDO client connection.
#[derive(Debug, Default)]
struct EmbeddedSdoConnection {
    /// The last successfully processed sequence number from the server.
    last_sequence_number: u8,
    /// The sequence number to use for our next request.
    next_sequence_number: u8,
    /// Queue of pending requests from the application.
    request_queue: VecDeque<SdoClientRequest>,
}

/// Manages all embedded SDO client channels (0x1280 - 0x12FF).
#[derive(Debug, Default)]
pub struct EmbeddedSdoClient {
    /// Maps a channel index (e.g., 0x1280) to its connection state.
    connections: BTreeMap<u16, EmbeddedSdoConnection>,
}

impl EmbeddedSdoClient {
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues an SDO Read request to be sent on a specific client channel.
    pub fn queue_read(
        &mut self,
        channel_index: u16,
        index: u16,
        sub_index: u8,
    ) -> Result<(), PowerlinkError> {
        if !(0x1280..=0x12FF).contains(&channel_index) {
            return Err(PowerlinkError::InternalError(
                "Invalid SDO client channel index",
            ));
        }
        let conn = self
            .connections
            .entry(channel_index)
            .or_insert_with(Default::default);

        conn.request_queue.push_back(SdoClientRequest {
            command_id: CommandId::ReadByIndex,
            index,
            sub_index,
            data: Vec::new(),
        });
        Ok(())
    }

    /// Queues an SDO Write request to be sent on a specific client channel.
    pub fn queue_write(
        &mut self,
        channel_index: u16,
        index: u16,
        sub_index: u8,
        data: Vec<u8>,
    ) -> Result<(), PowerlinkError> {
        if !(0x1280..=0x12FF).contains(&channel_index) {
            return Err(PowerlinkError::InternalError(
                "Invalid SDO client channel index",
            ));
        }
        let conn = self
            .connections
            .entry(channel_index)
            .or_insert_with(Default::default);

        conn.request_queue.push_back(SdoClientRequest {
            command_id: CommandId::WriteByIndex,
            index,
            sub_index,
            data,
        });
        Ok(())
    }

    /// Handles an incoming SDO response from an RPDO container.
    pub fn handle_response(&mut self, channel_index: u16, payload: &[u8]) {
        let Some(conn) = self.connections.get_mut(&channel_index) else {
            warn!(
                "[SDO-PDO] Client: Received response for unconfigured channel {:#06X}",
                channel_index
            );
            return;
        };

        let Ok(response) = PdoSdoCommand::deserialize(payload) else {
            warn!(
                "[SDO-PDO] Client: Failed to deserialize response for channel {:#06X}",
                channel_index
            );
            return;
        };

        // Check sequence number
        if response.sequence_header.sequence_number
            != conn.next_sequence_number.wrapping_sub(1) % 64
        {
            trace!(
                "[SDO-PDO] Client: Ignoring duplicate/old response for channel {:#06X}. Seq: {}",
                channel_index, response.sequence_header.sequence_number
            );
            return;
        }

        conn.last_sequence_number = response.sequence_header.sequence_number;

        // Handle response
        if response.is_aborted {
            let abort_code = u32::from_le_bytes(response.data[0..4].try_into().unwrap_or_default());
            error!(
                "[SDO-PDO] Client: Received SDO Abort on channel {:#06X}. Code: {:#010X}",
                channel_index, abort_code
            );
            // TODO: Notify application of failure
        } else {
            info!(
                "[SDO-PDO] Client: Received SDO Response on channel {:#06X}. Data: {:02X?}",
                channel_index, response.data
            );
            // TODO: Notify application of success with data
        }
    }

    /// Retrieves the pending request payload for a TPDO container.
    /// This consumes the request from the queue.
    pub fn get_pending_request(&mut self, channel_index: u16, container_len: usize) -> Vec<u8> {
        let conn = self
            .connections
            .entry(channel_index)
            .or_insert_with(Default::default);

        let Some(request) = conn.request_queue.pop_front() else {
            // No request, send NIL
            // TODO: Send a proper NIL command
            return vec![0; container_len];
        };

        let cmd = PdoSdoCommand {
            sequence_header: PdoSequenceLayerHeader {
                sequence_number: conn.next_sequence_number,
                connection_state: 2, // Connection valid
            },
            transaction_id: 1, // TODO: Manage TIDs
            is_response: false,
            is_aborted: false,
            segmentation: Segmentation::Expedited, // Always expedited
            valid_payload_length: 0,               // Will be set
            command_id: request.command_id,
            index: request.index,
            sub_index: request.sub_index,
            data: request.data,
        };

        conn.next_sequence_number = conn.next_sequence_number.wrapping_add(1) % 64;

        let mut payload = cmd.serialize();
        if payload.len() > container_len {
            error!(
                "[SDO-PDO] Client: Request for {:#06X} ({} bytes) exceeds container length ({} bytes). Dropping.",
                channel_index,
                payload.len(),
                container_len
            );
            // Re-queue the request? For now, just drop it.
            return vec![0; container_len];
        }

        payload.resize(container_len, 0); // Pad to fill container
        payload
    }
}
