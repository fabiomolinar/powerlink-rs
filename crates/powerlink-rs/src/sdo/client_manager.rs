// crates/powerlink-rs/src/sdo/client_manager.rs
//! Manages multiple, concurrent, stateful SDO client connections.
//!
//! This is primarily used by the Managing Node (MN) to perform complex
//! SDO transfers (like segmented downloads for CFM/PDL) to multiple CNs
//! simultaneously.

use crate::od::ObjectDictionary;
use crate::sdo::command::{CommandId, CommandLayerHeader, SdoCommand, Segmentation};
use crate::sdo::sequence::{ReceiveConnState, SendConnState, SequenceLayerHeader};
use crate::sdo::{OD_IDX_SDO_RETRIES, OD_IDX_SDO_TIMEOUT};
use crate::types::NodeId;
use crate::PowerlinkError;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use log::{debug, error, info, trace, warn};

const MAX_CLIENT_PAYLOAD: usize = 1452; // Max SDO payload (1456) - 4 byte command header

/// Internal state of a single SDO client-side transfer.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SdoClientConnectionState {
    /// No transfer active, awaiting request.
    Idle,
    /// Waiting for SDO Init response from server.
    Opening,
    /// Connection established, ready to send main command.
    Established,
    /// Sent an upload (read) request, awaiting first segment/response.
    UploadInit,
    /// Receiving a segmented upload from the server.
    UploadInProgress,
    /// Sending a segmented download to the server.
    DownloadInProgress,
    /// Transfer complete or aborted, waiting to be pruned.
    Closed,
}

/// Represents the state of a single SDO client connection to a specific CN.
#[derive(Debug, Clone)]
struct SdoClientConnection {
    /// The Node ID of the server (CN) we are talking to.
    target_node_id: NodeId,
    /// The current state of this connection.
    state: SdoClientConnectionState,
    /// The current transaction ID for this transfer.
    transaction_id: u8,
    /// The send sequence number (ssnr) we will use for the next frame.
    send_sequence_number: u8,
    /// The last receive sequence number (rsnr) we received from the server.
    last_received_sequence_number: u8,
    /// Buffer for segmented data (download: data to send; upload: data received).
    data_buffer: Vec<u8>,
    /// Current offset in the data_buffer.
    offset: usize,
    /// Total expected size of the transfer.
    total_size: usize,
    /// Timestamp of the next action deadline (e.g., timeout).
    deadline_us: Option<u64>,
    /// Retries left for the current action.
    retries_left: u32,
    /// The last command we sent, stored for retransmission.
    last_sent_command: Option<(SequenceLayerHeader, SdoCommand)>,
    /// The command to send *after* initialization is complete (e.g., ReadByIndex).
    pending_command: Option<SdoCommand>,
}

impl SdoClientConnection {
    fn new(target_node_id: NodeId) -> Self {
        Self {
            target_node_id,
            state: SdoClientConnectionState::Idle,
            transaction_id: 0,
            send_sequence_number: 0,
            last_received_sequence_number: 63, // Per spec, init at -1 (mod 64)
            data_buffer: Vec::new(),
            offset: 0,
            total_size: 0,
            deadline_us: None,
            retries_left: 0,
            last_sent_command: None,
            pending_command: None,
        }
    }

    /// Checks if the connection is idle or closed.
    fn is_idle(&self) -> bool {
        matches!(
            self.state,
            SdoClientConnectionState::Idle | SdoClientConnectionState::Closed
        )
    }

    /// Checks if the connection is closed.
    fn is_closed(&self) -> bool {
        matches!(self.state, SdoClientConnectionState::Closed)
    }

    /// Creates an SDO Abort command. Resets internal state to Closed.
    fn abort(&mut self, abort_code: u32) -> (SequenceLayerHeader, SdoCommand) {
        error!(
            "Aborting SDO client connection to Node {}, code: {:#010X}",
            self.target_node_id.0, abort_code
        );
        self.state = SdoClientConnectionState::Closed; // Set state to be pruned
        self.last_sent_command = None; // No more retransmissions
        self.deadline_us = None;

        let cmd = SdoCommand {
            header: CommandLayerHeader {
                transaction_id: self.transaction_id,
                is_response: false, // This is a request
                is_aborted: true,
                segmentation: Segmentation::Expedited,
                command_id: CommandId::Nil,
                segment_size: 4, // Size of the abort code
            },
            data_size: None,
            payload: abort_code.to_le_bytes().to_vec(),
        };
        let seq = SequenceLayerHeader {
            send_sequence_number: self.send_sequence_number,
            send_con: SendConnState::NoConnection, // Closing connection
            receive_sequence_number: self.last_received_sequence_number,
            receive_con: ReceiveConnState::ConnectionValid, // Acknowledge last received frame
        };
        (seq, cmd)
    }

    /// Handles an SDO response frame from the server.
    fn handle_response(
        &mut self,
        seq_header: &SequenceLayerHeader,
        cmd: &SdoCommand,
        // current_time_us: u64, // No longer needed
        // od: &ObjectDictionary, // No longer needed
    ) {
        // --- 1. Validate Sequence ACKs ---
        if self.last_sent_command.is_none() {
            // We weren't waiting for a response.
            // This could be an old, duplicated frame from the server.
            warn!(
                "SDO Client: Received unexpected response from Node {}. Ignoring.",
                self.target_node_id.0
            );
            return;
        }

        let (last_seq, _last_cmd) = self.last_sent_command.as_ref().unwrap();
        if seq_header.receive_sequence_number != last_seq.send_sequence_number {
            warn!(
                "SDO Client: Server ACK mismatch. Expected {}, got {}. Ignoring.",
                last_seq.send_sequence_number, seq_header.receive_sequence_number
            );
            return;
        }

        // ACK is valid. Clear timeout and retransmission buffer.
        trace!("SDO Client: Received valid ACK for Seq {}.", last_seq.send_sequence_number);
        self.last_sent_command = None;
        self.deadline_us = None;
        self.retries_left = 0;

        // --- 2. Validate Server Sequence Number ---
        let expected_server_seq = self.last_received_sequence_number.wrapping_add(1) % 64;
        if seq_header.send_sequence_number == self.last_received_sequence_number {
            debug!(
                "SDO Client: Received duplicate frame from server (Seq: {}).",
                seq_header.send_sequence_number
            );
            // We already cleared our last_sent_command, so get_pending_request
            // will trigger an ACK retransmission.
            return;
        } else if seq_header.send_sequence_number != expected_server_seq {
            error!(
                "SDO Client: Server sequence mismatch. Expected {}, got {}. Aborting.",
                expected_server_seq, seq_header.send_sequence_number
            );
            // TODO: How to send an abort here? This fn doesn't return a command.
            // For now, just close the connection.
            self.state = SdoClientConnectionState::Closed;
            return;
        }

        // Server sequence number is valid.
        self.last_received_sequence_number = seq_header.send_sequence_number;

        // --- 3. Handle Server Aborts ---
        if cmd.header.is_aborted {
            let abort_code =
                u32::from_le_bytes(cmd.payload[0..4].try_into().unwrap_or_default());
            error!(
                "SDO Client: Server at Node {} aborted transfer (TID {}) with code {:#010X}",
                self.target_node_id.0, cmd.header.transaction_id, abort_code
            );
            self.state = SdoClientConnectionState::Closed;
            return;
        }

        // --- 4. Process by State ---
        match self.state {
            SdoClientConnectionState::Opening => {
                if seq_header.receive_con == ReceiveConnState::Initialization
                    && seq_header.send_con == SendConnState::Initialization
                {
                    info!(
                        "SDO Client: Connection to Node {} established.",
                        self.target_node_id.0
                    );
                    self.state = SdoClientConnectionState::Established;
                } else {
                    warn!("SDO Client: Invalid response in Opening state. Aborting.");
                    self.state = SdoClientConnectionState::Closed;
                }
            }
            SdoClientConnectionState::UploadInit => {
                // This is the response to our ReadByIndex command.
                match cmd.header.segmentation {
                    Segmentation::Expedited => {
                        info!(
                            "SDO Client: Expedited Read from Node {} complete ({} bytes).",
                            self.target_node_id.0,
                            cmd.payload.len()
                        );
                        // TODO: Return data to application? For now, just log.
                        trace!("Data: {:02X?}", cmd.payload);
                        self.state = SdoClientConnectionState::Closed;
                    }
                    Segmentation::Initiate => {
                        info!(
                            "SDO Client: Segmented Read from Node {} initiated ({} bytes total).",
                            self.target_node_id.0,
                            cmd.data_size.unwrap_or(0)
                        );
                        self.total_size = cmd.data_size.unwrap_or(0) as usize;
                        self.data_buffer.clear();
                        self.data_buffer.extend_from_slice(&cmd.payload);
                        self.offset = cmd.payload.len();
                        self.state = SdoClientConnectionState::UploadInProgress;
                        // get_pending_request will now send an ACK
                    }
                    _ => {
                        error!("SDO Client: Invalid segmentation in UploadInit state.");
                        self.state = SdoClientConnectionState::Closed;
                    }
                }
            }
            SdoClientConnectionState::UploadInProgress => {
                // This is a data segment from the server.
                if cmd.header.segmentation == Segmentation::Segment
                    || cmd.header.segmentation == Segmentation::Complete
                {
                    self.data_buffer.extend_from_slice(&cmd.payload);
                    self.offset += cmd.payload.len();

                    if cmd.header.segmentation == Segmentation::Complete {
                        info!(
                            "SDO Client: Segmented Read from Node {} complete ({} bytes).",
                            self.target_node_id.0, self.offset
                        );
                        self.state = SdoClientConnectionState::Closed;
                    }
                    // get_pending_request will send the next ACK
                } else {
                    error!(
                        "SDO Client: Expected segment, got {:?}. Aborting.",
                        cmd.header.segmentation
                    );
                    self.state = SdoClientConnectionState::Closed;
                }
            }
            SdoClientConnectionState::DownloadInProgress => {
                // This is an ACK for a segment we sent.
                if self.offset >= self.total_size {
                    info!(
                        "SDO Client: Segmented Write to Node {} complete.",
                        self.target_node_id.0
                    );
                    self.state = SdoClientConnectionState::Closed;
                }
                // get_pending_request will send the next segment.
            }
            _ => {
                warn!(
                    "SDO Client: Received unexpected SDO response in state {:?}",
                    self.state
                );
                self.state = SdoClientConnectionState::Closed;
            }
        }
    }

    /// Checks for timeouts and returns a frame to re-send if needed.
    fn tick(
        &mut self,
        current_time_us: u64,
        od: &ObjectDictionary,
    ) -> Option<(SequenceLayerHeader, SdoCommand)> {
        let Some(deadline) = self.deadline_us else {
            return None; // No timeout active
        };

        if current_time_us < deadline {
            return None; // Not yet timed out
        }

        // --- Timeout Occurred ---
        if self.retries_left > 0 {
            self.retries_left -= 1;
            warn!(
                "SDO Client: Timeout waiting for response from Node {}. Retrying ({} left).",
                self.target_node_id.0, self.retries_left
            );

            // Reschedule deadline
            let timeout_ms = od.read_u32(OD_IDX_SDO_TIMEOUT, 0).unwrap_or(15000) as u64;
            self.deadline_us = Some(current_time_us + timeout_ms * 1000);

            // Return the last sent command for retransmission
            // Note: We don't increment ssnr here
            self.last_sent_command.clone()
        } else {
            // No retries left, abort the connection
            error!(
                "SDO Client: No retries left for Node {}. Aborting.",
                self.target_node_id.0
            );
            Some(self.abort(0x0504_0000)) // SDO protocol timed out
        }
    }

    /// Gets the next command payload to send for this connection.
    fn get_pending_request(
        &mut self,
        current_time_us: u64,
        od: &ObjectDictionary,
    ) -> Option<(SequenceLayerHeader, SdoCommand)> {
        // Don't send a new request if we are waiting for an ACK
        if self.last_sent_command.is_some() {
            return None;
        }

        let timeout_ms = od.read_u32(OD_IDX_SDO_TIMEOUT, 0).unwrap_or(15000) as u64;
        let retries = od.read_u32(OD_IDX_SDO_RETRIES, 0).unwrap_or(2);

        let (seq, cmd) = match self.state {
            SdoClientConnectionState::Opening => {
                // Send Init request
                let seq = SequenceLayerHeader {
                    send_sequence_number: self.send_sequence_number,
                    send_con: SendConnState::Initialization,
                    receive_sequence_number: self.last_received_sequence_number,
                    receive_con: ReceiveConnState::NoConnection,
                };
                let cmd = SdoCommand {
                    header: CommandLayerHeader {
                        transaction_id: self.transaction_id,
                        ..Default::default()
                    },
                    data_size: None,
                    payload: Vec::new(),
                };
                (seq, cmd)
            }
            SdoClientConnectionState::Established => {
                // Send the main queued command
                if let Some(cmd) = self.pending_command.take() {
                    // This is a Read request
                    self.state = SdoClientConnectionState::UploadInit;
                    let seq = SequenceLayerHeader {
                        send_sequence_number: self.send_sequence_number,
                        send_con: SendConnState::ConnectionValidAckRequest, // Request ACK
                        receive_sequence_number: self.last_received_sequence_number,
                        receive_con: ReceiveConnState::ConnectionValid,
                    };
                    (seq, cmd)
                } else if !self.data_buffer.is_empty() {
                    // This is the start of a Write request
                    let (cmd, is_last) = self.get_next_download_segment();
                    self.state = if is_last {
                        SdoClientConnectionState::DownloadInProgress // Will transition to Closed on ACK
                    } else {
                        SdoClientConnectionState::DownloadInProgress
                    };
                    let seq = SequenceLayerHeader {
                        send_sequence_number: self.send_sequence_number,
                        send_con: SendConnState::ConnectionValidAckRequest, // Request ACK
                        receive_sequence_number: self.last_received_sequence_number,
                        receive_con: ReceiveConnState::ConnectionValid,
                    };
                    (seq, cmd)
                } else {
                    return None; // Nothing to do
                }
            }
            SdoClientConnectionState::UploadInProgress => {
                // We received a segment, now we send an ACK (NIL command)
                let seq = SequenceLayerHeader {
                    send_sequence_number: self.send_sequence_number,
                    send_con: SendConnState::ConnectionValid, // Just a simple ACK
                    receive_sequence_number: self.last_received_sequence_number,
                    receive_con: ReceiveConnState::ConnectionValid,
                };
                let cmd = SdoCommand {
                    header: CommandLayerHeader {
                        transaction_id: self.transaction_id,
                        ..Default::default()
                    },
                    data_size: None,
                    payload: Vec::new(),
                };
                (seq, cmd)
            }
            SdoClientConnectionState::DownloadInProgress => {
                // We received an ACK, send the next segment
                let (cmd, is_last) = self.get_next_download_segment();
                if is_last {
                    // State remains DownloadInProgress until we get the *final* ACK
                    trace!("SDO Client: Sending last download segment.");
                }
                let seq = SequenceLayerHeader {
                    send_sequence_number: self.send_sequence_number,
                    send_con: SendConnState::ConnectionValidAckRequest, // Request ACK
                    receive_sequence_number: self.last_received_sequence_number,
                    receive_con: ReceiveConnState::ConnectionValid,
                };
                (seq, cmd)
            }
            _ => return None, // Idle, Closed, UploadInit (waiting)
        };

        // Set deadline and store command for retransmission
        self.deadline_us = Some(current_time_us + timeout_ms * 1000);
        self.retries_left = retries;
        self.last_sent_command = Some((seq, cmd.clone()));

        // Increment send sequence number *after* storing it
        self.send_sequence_number = self.send_sequence_number.wrapping_add(1) % 64;

        Some((seq, cmd))
    }

    /// Internal helper to create the next SDO command for a segmented download.
    fn get_next_download_segment(&mut self) -> (SdoCommand, bool) {
        let is_initiate = self.offset == 0;
        let remaining = self.total_size.saturating_sub(self.offset);
        
        let (header_data_len, data_only_len) = if is_initiate {
            // For initiate, the "payload" is the whole buffer: [index/subindex(4), data...]
            (self.total_size, self.total_size - 4)
        } else {
            // For segments, the "payload" is just data
            (remaining, remaining)
        };

        let chunk_size = MAX_CLIENT_PAYLOAD.min(header_data_len);
        let data_end_offset = self.offset + chunk_size;
        let chunk = &self.data_buffer[self.offset..data_end_offset];

        let segmentation = if is_initiate {
            if self.total_size <= (MAX_CLIENT_PAYLOAD + 4) {
                // +4 for index/subindex header
                Segmentation::Expedited
            } else {
                Segmentation::Initiate
            }
        } else if remaining <= MAX_CLIENT_PAYLOAD {
            Segmentation::Complete
        } else {
            Segmentation::Segment
        };

        self.offset = data_end_offset;
        let is_last = self.offset >= self.total_size;

        let cmd = SdoCommand {
            header: CommandLayerHeader {
                transaction_id: self.transaction_id,
                is_response: false,
                is_aborted: false,
                segmentation,
                // TODO: This assumes WriteByIndex. This logic needs to be
                // generalized if we support other download commands.
                command_id: CommandId::WriteByIndex,
                segment_size: chunk.len() as u16,
            },
            data_size: if is_initiate {
                Some(data_only_len as u32) // Data size is *without* index/subindex
            } else {
                None
            },
            payload: chunk.to_vec(),
        };

        (cmd, is_last)
    }

    /// Initiates a new "Read Object" transfer.
    fn read_object(
        &mut self,
        index: u16,
        sub_index: u8,
        tid: u8,
        current_time_us: u64,
        od: &ObjectDictionary,
    ) -> Result<(), PowerlinkError> {
        if !self.is_idle() {
            return Err(PowerlinkError::SdoSequenceError("Client is busy"));
        }
        self.state = SdoClientConnectionState::Opening;
        self.transaction_id = tid;
        self.send_sequence_number = 0;
        self.last_received_sequence_number = 63;
        self.data_buffer.clear();
        self.offset = 0;
        self.total_size = 0;
        self.last_sent_command = None;

        // Create the ReadByIndex command that will be sent *after* init
        let cmd = SdoCommand {
            header: CommandLayerHeader {
                transaction_id: tid,
                segmentation: Segmentation::Expedited, // This is a request, always expedited
                command_id: CommandId::ReadByIndex,
                segment_size: 4, // Index(2) + SubIndex(1) + Reserved(1)
                ..Default::default()
            },
            data_size: None,
            payload: [index.to_le_bytes().as_slice(), &[sub_index, 0u8]].concat(),
        };
        self.pending_command = Some(cmd);

        // Set deadline for the *first* frame (Init)
        let timeout_ms = od.read_u32(OD_IDX_SDO_TIMEOUT, 0).unwrap_or(15000) as u64;
        self.deadline_us = Some(current_time_us + timeout_ms * 1000);
        self.retries_left = od.read_u32(OD_IDX_SDO_RETRIES, 0).unwrap_or(2);

        Ok(())
    }

    /// Initiates a new "Write Object" transfer.
    fn write_object(
        &mut self,
        index: u16,
        sub_index: u8,
        data: Vec<u8>,
        tid: u8,
        current_time_us: u64,
        od: &ObjectDictionary,
    ) -> Result<(), PowerlinkError> {
        if !self.is_idle() {
            return Err(PowerlinkError::SdoSequenceError("Client is busy"));
        }
        self.state = SdoClientConnectionState::Opening;
        self.transaction_id = tid;
        self.send_sequence_number = 0;
        self.last_received_sequence_number = 63;

        // Create the full payload for the *first* segment
        // [index(2), sub_index(1), reserved(1), data...]
        let mut first_payload = Vec::with_capacity(4 + data.len());
        first_payload.extend_from_slice(&index.to_le_bytes());
        first_payload.push(sub_index);
        first_payload.push(0u8); // Reserved byte
        first_payload.extend_from_slice(&data);

        self.data_buffer = first_payload;
        self.total_size = self.data_buffer.len();
        self.offset = 0;
        self.pending_command = None; // No pending command, data is in buffer
        self.last_sent_command = None;

        // Set deadline for the *first* frame (Init)
        let timeout_ms = od.read_u32(OD_IDX_SDO_TIMEOUT, 0).unwrap_or(15000) as u64;
        self.deadline_us = Some(current_time_us + timeout_ms * 1000);
        self.retries_left = od.read_u32(OD_IDX_SDO_RETRIES, 0).unwrap_or(2);

        Ok(())
    }
}

/// Manages all stateful SDO client connections for an MN.
#[derive(Debug, Default)]
pub struct SdoClientManager {
    connections: BTreeMap<NodeId, SdoClientConnection>,
    next_transaction_id: u8,
}

impl SdoClientManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Gets the next available transaction ID.
    fn get_next_tid(&mut self) -> u8 {
        self.next_transaction_id = self.next_transaction_id.wrapping_add(1);
        // Reserve 0 for special cases if needed?
        if self.next_transaction_id == 0 {
            self.next_transaction_id = 1;
        }
        self.next_transaction_id
    }

    /// Returns the absolute timestamp of the next SDO timeout, if any.
    /// This is the missing function required by `mn/main.rs`.
    pub fn next_action_time(&self, _od: &ObjectDictionary) -> Option<u64> {
        // Iterate over all handlers and find the minimum Some(deadline)
        self.connections
            .values()
            .filter_map(|conn| conn.deadline_us)
            .min()
    }

    /// Initiates a "Read Object" SDO transfer to a target CN.
    pub fn read_object_by_index(
        &mut self,
        target: NodeId,
        index: u16,
        sub_index: u8,
        current_time_us: u64,
        od: &ObjectDictionary,
    ) -> Result<(), PowerlinkError> {
        let tid = self.get_next_tid();
        let conn = self
            .connections
            .entry(target)
            .or_insert_with(|| SdoClientConnection::new(target));
        conn.read_object(index, sub_index, tid, current_time_us, od)
    }

    /// Initiates a "Write Object" SDO transfer to a target CN.
    pub fn write_object_by_index(
        &mut self,
        target: NodeId,
        index: u16,
        sub_index: u8,
        data: Vec<u8>,
        current_time_us: u64,
        od: &ObjectDictionary,
    ) -> Result<(), PowerlinkError> {
        let tid = self.get_next_tid();
        let conn = self
            .connections
            .entry(target)
            .or_insert_with(|| SdoClientConnection::new(target));
        conn.write_object(index, sub_index, data, tid, current_time_us, od)
    }

    /// Handles an incoming SDO response frame from a CN.
    pub fn handle_response(
        &mut self,
        source_node: NodeId,
        seq_header: SequenceLayerHeader,
        cmd: SdoCommand,
        // current_time_us: u64, // Removed
        // od: &ObjectDictionary,  // Removed
    ) {
        if let Some(conn) = self.connections.get_mut(&source_node) {
            conn.handle_response(&seq_header, &cmd);
            if conn.is_closed() {
                debug!("SDO connection to Node {} closed after response.", source_node.0);
                self.connections.remove(&source_node);
            }
        } else {
            warn!(
                "Received SDO response from Node {}, but have no active connection.",
                source_node.0
            );
        }
    }

    /// Ticks all active connections to check for timeouts.
    /// Returns the first retransmission/abort frame that needs to be sent.
    pub fn tick(
        &mut self,
        current_time_us: u64,
        od: &ObjectDictionary,
    ) -> Option<(NodeId, SequenceLayerHeader, SdoCommand)> {
        let mut response = None;
        let mut clients_to_prune = Vec::new();

        for (node_id, conn) in self.connections.iter_mut() {
            if response.is_none() {
                if let Some((seq, cmd)) = conn.tick(current_time_us, od) {
                    response = Some((*node_id, seq, cmd));
                }
            }
            if conn.is_closed() {
                clients_to_prune.push(*node_id);
            }
        }

        for node_id in clients_to_prune {
            debug!(
                "Pruning timed-out/closed SDO client connection for Node {}.",
                node_id.0
            );
            self.connections.remove(&node_id);
        }

        response
    }

    /// Gets the next queued ASnd SDO payload to be sent.
    /// This iterates through connections and lets them decide what to send next.
    pub fn get_pending_request(
        &mut self,
        current_time_us: u64,
        od: &ObjectDictionary,
    ) -> Option<(NodeId, SequenceLayerHeader, SdoCommand)> {
        // TODO: Implement round-robin or priority-based selection.
        // For now, just find the first connection that wants to send.
        let mut clients_to_prune = Vec::new();
        let mut response = None;

        for (node_id, conn) in self.connections.iter_mut() {
            if response.is_none() {
                if let Some((seq, cmd)) = conn.get_pending_request(current_time_us, od) {
                    response = Some((*node_id, seq, cmd));
                }
            }
            if conn.is_closed() {
                clients_to_prune.push(*node_id);
            }
        }

        for node_id in clients_to_prune {
            debug!(
                "Pruning closed SDO client connection for Node {} during send check.",
                node_id.0
            );
            self.connections.remove(&node_id);
        }

        response
    }
}