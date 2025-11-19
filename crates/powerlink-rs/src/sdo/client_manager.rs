// crates/powerlink-rs/src/sdo/client_manager.rs
//! Manages multiple, concurrent, stateful SDO client connections.
//!
//! This is primarily used by the Managing Node (MN) to perform complex
//! SDO transfers (like segmented downloads for CFM/PDL) to multiple CNs
//! simultaneously.

use crate::PowerlinkError;
use crate::od::ObjectDictionary;
use crate::sdo::command::{CommandId, CommandLayerHeader, SdoCommand, Segmentation};
use crate::sdo::sequence::{ReceiveConnState, SendConnState, SequenceLayerHeader};
use crate::sdo::{OD_IDX_SDO_RETRIES, OD_IDX_SDO_TIMEOUT};
use crate::types::NodeId;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use log::{debug, error, info, warn};

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

/// Represents a queued job for the SDO client.
#[derive(Debug, Clone)]
enum SdoJob {
    /// A sequence of writes defined by a Concise DCF binary stream.
    /// (Reference: EPSG DS 301, Section 6.7.2.2, Table 102)
    ConciseDcf {
        data: Vec<u8>,
        /// Current read offset in the data vector.
        offset: usize,
        /// Total number of entries (read from header).
        entries_remaining: u32,
    },
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

    /// The current job being processed.
    current_job: Option<SdoJob>,

    /// The pending command to send after connection establishment.
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
            current_job: None,
            pending_command: None,
        }
    }

    fn is_idle(&self) -> bool {
        matches!(
            self.state,
            SdoClientConnectionState::Idle | SdoClientConnectionState::Closed
        )
    }

    fn is_closed(&self) -> bool {
        matches!(self.state, SdoClientConnectionState::Closed)
    }

    fn abort(&mut self, abort_code: u32) -> (SequenceLayerHeader, SdoCommand) {
        error!(
            "Aborting SDO client connection to Node {}, code: {:#010X}",
            self.target_node_id.0, abort_code
        );
        self.state = SdoClientConnectionState::Closed;
        self.last_sent_command = None;
        self.deadline_us = None;
        self.current_job = None; // Abort the job

        let cmd = SdoCommand {
            header: CommandLayerHeader {
                transaction_id: self.transaction_id,
                is_response: false,
                is_aborted: true,
                segmentation: Segmentation::Expedited,
                command_id: CommandId::Nil,
                segment_size: 4,
            },
            data_size: None,
            payload: abort_code.to_le_bytes().to_vec(),
        };
        let seq = SequenceLayerHeader {
            send_sequence_number: self.send_sequence_number,
            send_con: SendConnState::NoConnection,
            receive_sequence_number: self.last_received_sequence_number,
            receive_con: ReceiveConnState::ConnectionValid,
        };
        (seq, cmd)
    }

    fn handle_response(&mut self, seq_header: &SequenceLayerHeader, cmd: &SdoCommand) {
        // 1. Validate Sequence ACKs
        if self.last_sent_command.is_none() {
            warn!(
                "SDO Client: Unexpected response from Node {}. Ignoring.",
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

        // ACK valid
        self.last_sent_command = None;
        self.deadline_us = None;
        self.retries_left = 0;

        // 2. Validate Server Sequence Number
        let expected_server_seq = self.last_received_sequence_number.wrapping_add(1) % 64;
        if seq_header.send_sequence_number == self.last_received_sequence_number {
            debug!("SDO Client: Duplicate frame from server. Ignoring.");
            return;
        } else if seq_header.send_sequence_number != expected_server_seq {
            error!("SDO Client: Sequence mismatch. Aborting.");
            self.state = SdoClientConnectionState::Closed;
            return;
        }
        self.last_received_sequence_number = seq_header.send_sequence_number;

        // 3. Handle Server Aborts
        if cmd.header.is_aborted {
            let abort_code = u32::from_le_bytes(cmd.payload[0..4].try_into().unwrap_or_default());
            error!(
                "SDO Client: Server Node {} aborted (TID {}) with code {:#010X}",
                self.target_node_id.0, cmd.header.transaction_id, abort_code
            );
            self.state = SdoClientConnectionState::Closed;
            self.current_job = None;
            return;
        }

        // 4. Process by State
        match self.state {
            SdoClientConnectionState::Opening => {
                if seq_header.receive_con == ReceiveConnState::Initialization
                    && seq_header.send_con == SendConnState::Initialization
                {
                    info!(
                        "SDO Client: Connection to Node {} established.",
                        self.target_node_id.0
                    );
                    // If we have a pending command (Read), send it.
                    // If we have data in buffer (Write), prepare to send it.
                    self.state = SdoClientConnectionState::Established;
                } else {
                    warn!("SDO Client: Invalid response in Opening. Aborting.");
                    self.state = SdoClientConnectionState::Closed;
                }
            }
            SdoClientConnectionState::DownloadInProgress => {
                if self.offset >= self.total_size {
                    info!(
                        "SDO Client: Write to Node {} complete.",
                        self.target_node_id.0
                    );

                    // Check if job has more commands
                    if self.prepare_next_job_command() {
                        // New command prepared in data_buffer.
                        // Stay in Established/DownloadInProgress to send next init frame.
                        self.state = SdoClientConnectionState::Established;
                    } else {
                        self.state = SdoClientConnectionState::Closed;
                    }
                }
            }
            // (Upload logic omitted for brevity)
            _ => {
                self.state = SdoClientConnectionState::Closed;
            }
        }
    }

    /// Parses the next command from the current job (if any).
    /// Sets up `data_buffer` and `total_size` for a Write operation.
    /// Returns true if a new command is ready to send, false if job is done.
    fn prepare_next_job_command(&mut self) -> bool {
        // We must use take() to mutate the job inside the Option, then put it back if not done
        let job = self.current_job.take();

        match job {
            Some(SdoJob::ConciseDcf {
                data,
                mut offset,
                mut entries_remaining,
            }) => {
                if entries_remaining == 0 {
                    return false;
                }

                // Parse Concise DCF Entry (Table 102)
                // Index(2) + Sub(1) + Size(4) + Data(...)
                if offset + 7 > data.len() {
                    error!("[SDO] Concise DCF buffer underrun (header).");
                    return false;
                }

                let index_bytes: [u8; 2] = data[offset..offset + 2].try_into().unwrap();
                let index = u16::from_le_bytes(index_bytes);
                let sub_index = data[offset + 2];
                let size_bytes: [u8; 4] = data[offset + 3..offset + 7].try_into().unwrap();
                let data_size = u32::from_le_bytes(size_bytes) as usize;
                offset += 7;

                if offset + data_size > data.len() {
                    error!("[SDO] Concise DCF buffer underrun (data).");
                    return false;
                }

                let param_data = &data[offset..offset + data_size];

                // Prepare data buffer for Write logic: [index(2), sub(1), reserved(1), data...]
                let mut full_payload = Vec::with_capacity(4 + param_data.len());
                full_payload.extend_from_slice(&index.to_le_bytes());
                full_payload.push(sub_index);
                full_payload.push(0); // Reserved
                full_payload.extend_from_slice(param_data);

                self.data_buffer = full_payload;
                self.total_size = self.data_buffer.len();
                self.offset = 0;

                // Update job state
                offset += data_size;
                entries_remaining -= 1;

                // Put the job back with updated offset
                self.current_job = Some(SdoJob::ConciseDcf {
                    data,
                    offset,
                    entries_remaining,
                });

                self.transaction_id = self.transaction_id.wrapping_add(1);
                if self.transaction_id == 0 {
                    self.transaction_id = 1;
                }

                info!(
                    "[SDO] Job: Configured Write 0x{:04X}/{} ({} bytes). Remaining: {}",
                    index, sub_index, data_size, entries_remaining
                );
                true
            }
            _ => false, // Single jobs are done after one pass or None
        }
    }

    fn tick(
        &mut self,
        current_time_us: u64,
        od: &ObjectDictionary,
    ) -> Option<(SequenceLayerHeader, SdoCommand)> {
        let Some(deadline) = self.deadline_us else {
            return None;
        };
        if current_time_us < deadline {
            return None;
        }

        if self.retries_left > 0 {
            self.retries_left -= 1;
            warn!(
                "SDO Client: Timeout Node {}. Retrying ({} left).",
                self.target_node_id.0, self.retries_left
            );
            let timeout_ms = od.read_u32(OD_IDX_SDO_TIMEOUT, 0).unwrap_or(15000) as u64;
            self.deadline_us = Some(current_time_us + timeout_ms * 1000);
            self.last_sent_command.clone()
        } else {
            error!(
                "SDO Client: Timeout Node {}. Aborting.",
                self.target_node_id.0
            );
            Some(self.abort(0x0504_0000))
        }
    }

    fn get_pending_request(
        &mut self,
        current_time_us: u64,
        od: &ObjectDictionary,
    ) -> Option<(SequenceLayerHeader, SdoCommand)> {
        if self.last_sent_command.is_some() {
            return None;
        }

        let timeout_ms = od.read_u32(OD_IDX_SDO_TIMEOUT, 0).unwrap_or(15000) as u64;
        let retries = od.read_u32(OD_IDX_SDO_RETRIES, 0).unwrap_or(2);

        let (seq, cmd) = match self.state {
            SdoClientConnectionState::Opening => {
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
                // If we have a job command pending (data in buffer), start sending
                if !self.data_buffer.is_empty() {
                    let (cmd, _is_last) = self.get_next_download_segment();
                    self.state = SdoClientConnectionState::DownloadInProgress;
                    let seq = SequenceLayerHeader {
                        send_sequence_number: self.send_sequence_number,
                        send_con: SendConnState::ConnectionValidAckRequest,
                        receive_sequence_number: self.last_received_sequence_number,
                        receive_con: ReceiveConnState::ConnectionValid,
                    };
                    (seq, cmd)
                } else if let Some(cmd) = self.pending_command.take() {
                    // Manual Read command
                    self.state = SdoClientConnectionState::UploadInit;
                    let seq = SequenceLayerHeader {
                        send_sequence_number: self.send_sequence_number,
                        send_con: SendConnState::ConnectionValidAckRequest,
                        receive_sequence_number: self.last_received_sequence_number,
                        receive_con: ReceiveConnState::ConnectionValid,
                    };
                    (seq, cmd)
                } else {
                    // No manual command and no buffer data. Check for next job command.
                    if self.prepare_next_job_command() {
                        // Recursive call to handle the new command immediately
                        return self.get_pending_request(current_time_us, od);
                    }
                    // Job done
                    self.state = SdoClientConnectionState::Closed;
                    return None;
                }
            }
            SdoClientConnectionState::DownloadInProgress => {
                let (cmd, _) = self.get_next_download_segment();
                let seq = SequenceLayerHeader {
                    send_sequence_number: self.send_sequence_number,
                    send_con: SendConnState::ConnectionValidAckRequest,
                    receive_sequence_number: self.last_received_sequence_number,
                    receive_con: ReceiveConnState::ConnectionValid,
                };
                (seq, cmd)
            }
            _ => return None,
        };

        self.deadline_us = Some(current_time_us + timeout_ms * 1000);
        self.retries_left = retries;
        self.last_sent_command = Some((seq, cmd.clone()));
        self.send_sequence_number = self.send_sequence_number.wrapping_add(1) % 64;

        Some((seq, cmd))
    }

    fn get_next_download_segment(&mut self) -> (SdoCommand, bool) {
        let is_initiate = self.offset == 0;
        let remaining = self.total_size.saturating_sub(self.offset);
        let (header_data_len, data_only_len) = if is_initiate {
            (self.total_size, self.total_size - 4)
        } else {
            (remaining, remaining)
        };

        let chunk_size = MAX_CLIENT_PAYLOAD.min(header_data_len);
        let data_end_offset = self.offset + chunk_size;
        let chunk = &self.data_buffer[self.offset..data_end_offset];

        let segmentation = if is_initiate {
            if self.total_size <= (MAX_CLIENT_PAYLOAD + 4) {
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
                segmentation,
                command_id: CommandId::WriteByIndex,
                segment_size: chunk.len() as u16,
                ..Default::default()
            },
            data_size: if is_initiate {
                Some(data_only_len as u32)
            } else {
                None
            },
            payload: chunk.to_vec(),
        };
        (cmd, is_last)
    }

    fn start_concise_dcf_job(
        &mut self,
        dcf_data: Vec<u8>,
        tid: u8,
        current_time_us: u64,
        od: &ObjectDictionary,
    ) -> Result<(), PowerlinkError> {
        if !self.is_idle() {
            return Err(PowerlinkError::SdoSequenceError("Client is busy"));
        }

        // Parse entry count from first 4 bytes (U32 LE)
        if dcf_data.len() < 4 {
            return Err(PowerlinkError::ValidationError("Concise DCF too short"));
        }
        let entries = u32::from_le_bytes(dcf_data[0..4].try_into().unwrap());

        self.state = SdoClientConnectionState::Opening;
        self.transaction_id = tid;
        self.send_sequence_number = 0;
        self.last_received_sequence_number = 63;

        self.current_job = Some(SdoJob::ConciseDcf {
            data: dcf_data,
            offset: 4, // Skip entry count
            entries_remaining: entries,
        });

        // Trigger first command load
        self.data_buffer.clear(); // Clear previous
        self.prepare_next_job_command(); // Load first command into buffer

        // Set timeout for Init frame
        let timeout_ms = od.read_u32(OD_IDX_SDO_TIMEOUT, 0).unwrap_or(15000) as u64;
        self.deadline_us = Some(current_time_us + timeout_ms * 1000);
        self.retries_left = od.read_u32(OD_IDX_SDO_RETRIES, 0).unwrap_or(2);
        Ok(())
    }

    fn start_read_job(
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

        // Prepare Read Command
        let cmd = SdoCommand {
            header: CommandLayerHeader {
                transaction_id: tid,
                segmentation: Segmentation::Expedited,
                command_id: CommandId::ReadByIndex,
                segment_size: 4,
                ..Default::default()
            },
            data_size: None,
            payload: [index.to_le_bytes().as_slice(), &[sub_index, 0u8]].concat(),
        };
        self.pending_command = Some(cmd);

        let timeout_ms = od.read_u32(OD_IDX_SDO_TIMEOUT, 0).unwrap_or(15000) as u64;
        self.deadline_us = Some(current_time_us + timeout_ms * 1000);
        self.retries_left = od.read_u32(OD_IDX_SDO_RETRIES, 0).unwrap_or(2);
        Ok(())
    }

    fn start_write_job(
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

        // Create payload for WriteByIndex [index(2), sub(1), reserved(1), data...]
        let mut full_payload = Vec::with_capacity(4 + data.len());
        full_payload.extend_from_slice(&index.to_le_bytes());
        full_payload.push(sub_index);
        full_payload.push(0);
        full_payload.extend_from_slice(&data);

        self.data_buffer = full_payload;
        self.total_size = self.data_buffer.len();
        self.offset = 0;

        let timeout_ms = od.read_u32(OD_IDX_SDO_TIMEOUT, 0).unwrap_or(15000) as u64;
        self.deadline_us = Some(current_time_us + timeout_ms * 1000);
        self.retries_left = od.read_u32(OD_IDX_SDO_RETRIES, 0).unwrap_or(2);
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct SdoClientManager {
    connections: BTreeMap<NodeId, SdoClientConnection>,
    next_transaction_id: u8,
}

impl SdoClientManager {
    pub fn new() -> Self {
        Self::default()
    }

    fn get_next_tid(&mut self) -> u8 {
        self.next_transaction_id = self.next_transaction_id.wrapping_add(1);
        if self.next_transaction_id == 0 {
            self.next_transaction_id = 1;
        }
        self.next_transaction_id
    }

    pub fn next_action_time(&self, _od: &ObjectDictionary) -> Option<u64> {
        self.connections
            .values()
            .filter_map(|c| c.deadline_us)
            .min()
    }

    /// Starts a configuration download job (Concise DCF) for the target node.
    pub fn start_configuration_download(
        &mut self,
        target: NodeId,
        dcf_data: Vec<u8>,
        current_time_us: u64,
        od: &ObjectDictionary,
    ) -> Result<(), PowerlinkError> {
        let tid = self.get_next_tid();
        let conn = self
            .connections
            .entry(target)
            .or_insert_with(|| SdoClientConnection::new(target));
        conn.start_concise_dcf_job(dcf_data, tid, current_time_us, od)
    }

    pub fn read_object_by_index(
        &mut self,
        target: NodeId,
        index: u16,
        sub_index: u8,
        time: u64,
        od: &ObjectDictionary,
    ) -> Result<(), PowerlinkError> {
        let tid = self.get_next_tid();
        let conn = self
            .connections
            .entry(target)
            .or_insert_with(|| SdoClientConnection::new(target));
        conn.start_read_job(index, sub_index, tid, time, od)
    }

    pub fn write_object_by_index(
        &mut self,
        target: NodeId,
        index: u16,
        sub_index: u8,
        data: Vec<u8>,
        time: u64,
        od: &ObjectDictionary,
    ) -> Result<(), PowerlinkError> {
        let tid = self.get_next_tid();
        let conn = self
            .connections
            .entry(target)
            .or_insert_with(|| SdoClientConnection::new(target));
        conn.start_write_job(index, sub_index, data, tid, time, od)
    }

    pub fn handle_response(&mut self, source: NodeId, seq: SequenceLayerHeader, cmd: SdoCommand) {
        if let Some(conn) = self.connections.get_mut(&source) {
            conn.handle_response(&seq, &cmd);
            if conn.is_closed() {
                self.connections.remove(&source);
            }
        }
    }

    pub fn tick(
        &mut self,
        time: u64,
        od: &ObjectDictionary,
    ) -> Option<(NodeId, SequenceLayerHeader, SdoCommand)> {
        let mut res = None;
        let mut prune = Vec::new();
        for (id, conn) in self.connections.iter_mut() {
            if res.is_none() {
                if let Some(out) = conn.tick(time, od) {
                    res = Some((*id, out.0, out.1));
                }
            }
            if conn.is_closed() {
                prune.push(*id);
            }
        }
        for id in prune {
            self.connections.remove(&id);
        }
        res
    }

    pub fn get_pending_request(
        &mut self,
        time: u64,
        od: &ObjectDictionary,
    ) -> Option<(NodeId, SequenceLayerHeader, SdoCommand)> {
        let mut res = None;
        let mut prune = Vec::new();
        for (id, conn) in self.connections.iter_mut() {
            if res.is_none() {
                if let Some(out) = conn.get_pending_request(time, od) {
                    res = Some((*id, out.0, out.1));
                }
            }
            if conn.is_closed() {
                prune.push(*id);
            }
        }
        for id in prune {
            self.connections.remove(&id);
        }
        res
    }
}
