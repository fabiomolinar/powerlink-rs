use crate::PowerlinkError;
use crate::frame::PRFlag;
use crate::od::{ObjectDictionary, ObjectValue};
use crate::sdo::command::{
    CommandId, CommandLayerHeader, ReadByIndexRequest, SdoCommand, Segmentation,
    WriteByIndexRequest,
};
use crate::sdo::sequence::{ReceiveConnState, SendConnState, SequenceLayerHeader};
use crate::sdo::state::{SdoServerState, SdoTransferState};
use alloc::vec;
use alloc::vec::Vec;
use log::{debug, error, trace, warn, info};

/// Manages a single SDO server connection.
///
/// A full implementation would use a BTreeMap or similar to manage multiple
/// connections, keyed by a client identifier (like NodeId or a socket address).
/// For now, this struct manages a single connection for simplicity.
pub struct SdoServer {
    state: SdoServerState,
    // The next sequence number this server will send.
    pub(super) send_sequence_number: u8,
    // The last sequence number the server correctly received from the client.
    pub(super) last_received_sequence_number: u8,
    // A queue for pending client requests (outgoing SDO frames) initiated by this node's application.
    pub(super) pending_client_requests: Vec<Vec<u8>>,
}

const MAX_EXPEDITED_PAYLOAD: usize = 1452; // Max SDO payload within ASnd frame (excluding headers)
const OD_IDX_SDO_TIMEOUT: u16 = 0x1300;
const OD_IDX_SDO_RETRIES: u16 = 0x1302;

impl SdoServer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues an SDO request payload to be sent to the MN.
    /// This would be called by the application logic.
    pub fn queue_request(&mut self, payload: Vec<u8>) {
        self.pending_client_requests.push(payload);
    }

    /// Retrieves and removes the next pending client request from the queue.
    pub fn pop_pending_request(&mut self) -> Option<Vec<u8>> {
        if self.pending_client_requests.is_empty() {
            None
        } else {
            // Treat the Vec as a FIFO queue for simplicity.
            Some(self.pending_client_requests.remove(0))
        }
    }

    /// Checks for pending client (outgoing) requests and returns their count and priority.
    /// This is used to set the RS/PR flags in PRes frames.
    pub fn pending_request_count_and_priority(&self) -> (u8, PRFlag) {
        let count = self.pending_client_requests.len();
        if count > 0 {
            // SDO via ASnd uses PRIO_GENERIC_REQUEST.
            // A real implementation would check the priority of each pending request.
            (count.min(7) as u8, PRFlag::PrioGenericRequest)
        } else {
            (0, PRFlag::default())
        }
    }

    /// Returns the absolute timestamp of the next SDO timeout, if any.
    pub fn next_action_time(&self) -> Option<u64> {
        if let SdoServerState::SegmentedUpload(state) = &self.state {
            state.deadline_us
        } else {
            None
        }
    }

    /// Handles time-based events for the SDO server, like retransmission timeouts.
    pub fn tick(
        &mut self,
        current_time_us: u64,
        od: &ObjectDictionary,
    ) -> Result<Option<Vec<u8>>, PowerlinkError> {
        let mut retransmit_command = None;
        let mut abort_params: Option<(u8, u32)> = None;

        if let SdoServerState::SegmentedUpload(state) = &mut self.state {
            if let Some(deadline) = state.deadline_us {
                if current_time_us >= deadline {
                    // Timeout occurred!
                    if state.retransmissions_left > 0 {
                        state.retransmissions_left -= 1;
                        warn!(
                            "[SDO] Server: Segment ACK timeout for TID {}. Retransmitting ({} retries left).",
                            state.transaction_id, state.retransmissions_left
                        );

                        // Reschedule the next timeout
                        let timeout_ms = od.read_u32(OD_IDX_SDO_TIMEOUT, 0).unwrap_or(15000) as u64;
                        state.deadline_us = Some(current_time_us + timeout_ms * 1000);

                        // Retransmit the last sent segment.
                        if let Some(last_command) = &state.last_sent_segment {
                            retransmit_command = Some(last_command.clone());
                        } else {
                            // This should not happen if we are in this state.
                            return Err(PowerlinkError::InternalError("Missing last sent segment during retransmission"));
                        }
                    } else {
                        // No retransmissions left, abort the connection.
                        error!("[SDO] Server: No retransmissions left for TID {}. Aborting connection.", state.transaction_id);
                        abort_params = Some((state.transaction_id, 0x0504_0000)); // SDO protocol timed out
                    }
                }
            }
        }

        if let Some(command) = retransmit_command {
            let response_header = SequenceLayerHeader {
                receive_sequence_number: self.last_received_sequence_number,
                receive_con: ReceiveConnState::ConnectionValid,
                send_sequence_number: self.send_sequence_number, // Use the same sequence number
                send_con: SendConnState::ConnectionValidAckRequest, // Request ACK again
            };
            return self.serialize_sdo_response(response_header, command).map(Some);
        }

        if let Some((tid, code)) = abort_params {
            let abort_command = self.abort(tid, code);
            let response_header = SequenceLayerHeader {
                receive_sequence_number: self.last_received_sequence_number,
                receive_con: ReceiveConnState::ConnectionValid,
                send_sequence_number: self.send_sequence_number,
                send_con: SendConnState::NoConnection, // Closing connection
            };
            return self.serialize_sdo_response(response_header, abort_command).map(Some);
        }

        Ok(None)
    }


    /// Processes an incoming SDO request contained within an ASnd payload.
    ///
    /// This function handles the Sequence Layer logic and will eventually delegate
    /// the inner command to a Command Layer processor.
    /// The input `request_sdo_payload` should start directly with the SDO Sequence Layer header.
    pub fn handle_request(
        &mut self,
        request_sdo_payload: &[u8], // Renamed to clarify it's SDO Seq+Cmd+Data
        od: &mut ObjectDictionary,
        current_time_us: u64,
    ) -> Result<Vec<u8>, PowerlinkError> {
        // SDO Sequence Layer Header starts at offset 0 of the ASnd payload.
        // SDO Command Layer Header starts at offset 4.
        if request_sdo_payload.len() < 4 {
            // Need at least a sequence header
            return Err(PowerlinkError::BufferTooShort);
        }
        trace!("Handling SDO request payload: {:?}", request_sdo_payload);
        // Use the inherent deserialize method (now unambiguous)
        let sequence_header = SequenceLayerHeader::deserialize(&request_sdo_payload[0..4])?;
        let command_payload = &request_sdo_payload[4..]; // Command Layer starts after Seq Layer

        debug!("Parsed SDO sequence header: {:?}", sequence_header);

        let mut response_header = self.process_sequence_layer(sequence_header)?;

        // Command payload can be empty (e.g. SDO Init ACK, NIL command)
        if command_payload.is_empty()
            && (self.state == SdoServerState::Opening
                || sequence_header.send_con == SendConnState::ConnectionValid)
        {
            // This is likely an ACK for our Init, or a NIL command.
            // In the case of Opening, process_sequence_layer already moved us to Established.
            // In the case of a NIL command, we just need to send an ACK.
            debug!("Received ACK or NIL command.");

            // If we are in a segmented upload, the ACK should trigger the next segment
            if let SdoServerState::SegmentedUpload(state) = core::mem::take(&mut self.state) {
                debug!("Client ACK received, continuing segmented upload.");
                let response_command = self.handle_segmented_upload(state, od, current_time_us);
                response_header.receive_sequence_number = self.last_received_sequence_number;
                return self.serialize_sdo_response(response_header, response_command);
            }

            // Otherwise, just send an empty ACK response
            let response_command = SdoCommand {
                header: CommandLayerHeader {
                    transaction_id: 0, // Transaction ID might not be present, use 0 for simple ACK
                    is_response: true,
                    ..Default::default()
                },
                data_size: None,
                payload: Vec::new(),
            };
            response_header.receive_sequence_number = self.last_received_sequence_number;
            return self.serialize_sdo_response(response_header, response_command);
        }

        if command_payload.is_empty() {
            error!("Received empty command payload in unexpected state.");
            return Err(PowerlinkError::InvalidPlFrame); // Or SdoSequenceError
        }

        // Use the inherent deserialize method (now unambiguous)
        let sdo_command = SdoCommand::deserialize(command_payload)?;

        debug!("Parsed SDO command: {:?}", sdo_command);

        // Process the command and generate a response command.
        let response_command = self.process_command_layer(sdo_command, od, current_time_us);
        debug!("Generated SDO response command: {:?}", response_command);

        // Acknowledge the received sequence number in the response.
        response_header.receive_sequence_number = self.last_received_sequence_number;

        self.serialize_sdo_response(response_header, response_command)
    }

    /// Helper to serialize the final SDO payload (Seq + Cmd)
    fn serialize_sdo_response(
        &self,
        seq_header: SequenceLayerHeader,
        cmd: SdoCommand,
    ) -> Result<Vec<u8>, PowerlinkError> {
        // --- Assemble the full SDO response payload (Seq + Cmd + Data) ---
        let mut response_sdo_payload = vec![0u8; 1500]; // Allocate max SDO size.
        // Use inherent serialize methods
        let seq_len = seq_header.serialize(&mut response_sdo_payload[0..4])?;
        let cmd_len = cmd.serialize(&mut response_sdo_payload[seq_len..])?;
        let total_sdo_len = seq_len + cmd_len;
        response_sdo_payload.truncate(total_sdo_len);

        Ok(response_sdo_payload) // Return only the SDO payload (Seq+Cmd+Data)
    }

    /// Processes the SDO command, interacts with the OD, and returns a response command.
    fn process_command_layer(
        &mut self,
        command: SdoCommand,
        od: &mut ObjectDictionary,
        current_time_us: u64,
    ) -> SdoCommand {
        // Temporarily take ownership of the state to avoid borrow checker issues.
        let current_state = core::mem::take(&mut self.state);

        // If we are in a segmented upload, any new valid command from the client
        // just serves as an ACK to trigger the next segment.
        if let SdoServerState::SegmentedUpload(state) = current_state {
            if state.transaction_id == command.header.transaction_id {
                debug!("Client ACK received, continuing segmented upload.");
                // Reset send sequence number on ACK according to state diagram logic
                // (though the logic is complex, this simplification works for basic ACK)
                // self.send_sequence_number = state.last_sent_seq_num; // Need to track last sent
                return self.handle_segmented_upload(state, od, current_time_us);
            }
            // If transaction ID doesn't match, restore state and fall through.
            error!(
                "Mismatched transaction ID during segmented upload. Expected {}, got {}",
                state.transaction_id, command.header.transaction_id
            );
            self.state = SdoServerState::SegmentedUpload(state);
            // Abort with general error if transaction IDs don't match mid-transfer
            return self.abort(command.header.transaction_id, 0x0800_0000);
        } else {
            // Not a segmented upload, restore state.
            self.state = current_state;
        }

        let mut response_header = CommandLayerHeader {
            transaction_id: command.header.transaction_id,
            is_response: true,
            is_aborted: false,
            segmentation: Segmentation::Expedited, // Default response is expedited
            command_id: command.header.command_id,
            segment_size: 0,
        };

        match command.header.command_id {
            CommandId::ReadByIndex => {
                match ReadByIndexRequest::from_payload(&command.payload) {
                    Ok(req) => {
                        info!(
                            "Processing SDO Read request for 0x{:04X}/{}",
                            req.index, req.sub_index
                        );
                        match od.read(req.index, req.sub_index) {
                            Some(value) => {
                                let payload = value.serialize();
                                if payload.len() <= MAX_EXPEDITED_PAYLOAD {
                                    // Expedited transfer
                                    info!(
                                        "Responding with expedited read of {} bytes.",
                                        payload.len()
                                    );
                                    response_header.segment_size = payload.len() as u16;
                                    SdoCommand {
                                        header: response_header,
                                        data_size: None,
                                        payload,
                                    }
                                } else {
                                    // Initiate segmented transfer
                                    info!(
                                        "Initiating segmented upload of {} bytes.",
                                        payload.len()
                                    );
                                    let transfer_state = SdoTransferState {
                                        transaction_id: command.header.transaction_id,
                                        total_size: payload.len(),
                                        data_buffer: payload,
                                        offset: 0, // Start sending from the beginning
                                        index: req.index,
                                        sub_index: req.sub_index,
                                        deadline_us: None,
                                        retransmissions_left: 0, // Will be set in handle_segmented_upload
                                        last_sent_segment: None,
                                    };
                                    // The handle_segmented_upload function now mutates self.state
                                    self.handle_segmented_upload(transfer_state, od, current_time_us)
                                }
                            }
                            None => self.abort(
                                command.header.transaction_id,
                                0x0602_0000, // Object does not exist
                            ),
                        }
                    }
                    Err(_) => self.abort(command.header.transaction_id, 0x0800_0000), // General error parsing payload
                }
            }
            CommandId::WriteByIndex => self.handle_write_by_index(command, response_header, od),
            CommandId::Nil => {
                // A NIL command acts as an ACK, often used during segmented uploads.
                // The main logic for handling this is already at the start of this function.
                // If we reach here, it means we're not in a segmented upload, so just send an empty ack.
                debug!("Received NIL command, sending empty ACK.");
                SdoCommand {
                    header: response_header,
                    data_size: None,
                    payload: Vec::new(),
                }
            }
            _ => {
                // Unsupported command
                error!(
                    "Unsupported SDO command received: {:?}",
                    command.header.command_id
                );
                self.abort(command.header.transaction_id, 0x0504_0001) // Command specifier not valid
            }
        }
    }

    /// Handles sending the next segment during an upload, or initiates the upload.
    /// Updates self.state.
    fn handle_segmented_upload(
        &mut self,
        mut state: SdoTransferState,
        od: &ObjectDictionary,
        current_time_us: u64,
    ) -> SdoCommand {
        let mut response_header = CommandLayerHeader {
            transaction_id: state.transaction_id,
            is_response: true,
            is_aborted: false,
            segmentation: Segmentation::Segment, // Default unless first or last
            command_id: CommandId::ReadByIndex,  // Response to a read request
            segment_size: 0,
        };

        let chunk_size = MAX_EXPEDITED_PAYLOAD; // Use the max allowed SDO data size
        let remaining = state.total_size.saturating_sub(state.offset);
        let current_chunk_size = chunk_size.min(remaining);
        // Clone the data slice to be sent.
        let chunk = state.data_buffer[state.offset..state.offset + current_chunk_size].to_vec();

        let data_size = if state.offset == 0 {
            // This is the first segment (Initiate)
            info!(
                "Sending Initiate Segmented Upload: total size {}",
                state.total_size
            );
            response_header.segmentation = Segmentation::Initiate;
            Some(state.total_size as u32)
        } else {
            None // Data size only in Initiate frame
        };

        // Update the offset for the *next* segment
        state.offset += current_chunk_size;
        debug!(
            "Sending upload segment: new offset={}, segment size={}",
            state.offset,
            chunk.len()
        );

        if state.offset >= state.total_size {
            // This is the last segment
            info!("Segmented upload complete.");
            response_header.segmentation = Segmentation::Complete;
            self.state = SdoServerState::Established; // Transition back after sending last segment
        } else {
            // More segments to follow, set up for retransmission
            let timeout_ms = od.read_u32(OD_IDX_SDO_TIMEOUT, 0).unwrap_or(15000) as u64;
            state.deadline_us = Some(current_time_us + timeout_ms * 1000);
            state.retransmissions_left = od.read_u32(OD_IDX_SDO_RETRIES, 0).unwrap_or(2);
            self.state = SdoServerState::SegmentedUpload(state);
        }

        response_header.segment_size = chunk.len() as u16;

        let command = SdoCommand {
            header: response_header,
            data_size,
            payload: chunk,
        };

        // Store the command for potential retransmission
        if let SdoServerState::SegmentedUpload(ref mut s) = self.state {
            s.last_sent_segment = Some(command.clone());
        }

        command
    }


    fn handle_write_by_index(
        &mut self,
        command: SdoCommand,
        response_header: CommandLayerHeader,
        od: &mut ObjectDictionary,
    ) -> SdoCommand {
        match command.header.segmentation {
            Segmentation::Expedited => {
                info!("Processing expedited SDO Write.");
                // Handle a complete write in a single frame.
                match WriteByIndexRequest::from_payload(&command.payload) {
                    Ok(req) => match self.write_to_od(req.index, req.sub_index, req.data, od) {
                        Ok(_) => SdoCommand {
                            header: response_header,
                            data_size: None,
                            payload: Vec::new(), // Successful write has empty payload
                        },
                        Err(abort_code) => self.abort(command.header.transaction_id, abort_code),
                    },
                    Err(_) => self.abort(command.header.transaction_id, 0x0800_0000), // General error parsing payload
                }
            }
            Segmentation::Initiate => {
                info!("Initiating segmented SDO download.");
                // Start a new segmented download.
                match WriteByIndexRequest::from_payload(&command.payload) {
                    Ok(req) => {
                        // Check if total size exceeds buffer limits (add later)
                        self.state = SdoServerState::SegmentedDownload(SdoTransferState {
                            transaction_id: command.header.transaction_id,
                            total_size: command.data_size.unwrap_or(0) as usize,
                            data_buffer: req.data.to_vec(), // Store first segment
                            offset: req.data.len(),         // Track bytes received
                            index: req.index,
                            sub_index: req.sub_index,
                            deadline_us: None,
                            retransmissions_left: 0,
                            last_sent_segment: None,
                        });
                        SdoCommand {
                            header: response_header, // Send ACK response
                            data_size: None,
                            payload: Vec::new(),
                        }
                    }
                    Err(_) => self.abort(command.header.transaction_id, 0x0800_0000), // General error parsing payload
                }
            }
            Segmentation::Segment | Segmentation::Complete => {
                // Borrow self.state mutably only within this block
                if let SdoServerState::SegmentedDownload(ref mut transfer_state) = self.state {
                    if transfer_state.transaction_id != command.header.transaction_id {
                        error!(
                            "Mismatched transaction ID during segmented download. Expected {}, got {}",
                            transfer_state.transaction_id, command.header.transaction_id
                        );
                        return self.abort(command.header.transaction_id, 0x0800_0000);
                        // General error
                    }

                    // Append received data
                    transfer_state
                        .data_buffer
                        .extend_from_slice(&command.payload);
                    transfer_state.offset += command.payload.len();
                    debug!(
                        "Received download segment: new offset={}",
                        transfer_state.offset
                    );

                    if command.header.segmentation == Segmentation::Complete {
                        info!("Segmented download complete, writing to OD.");
                        // Check if received size matches expected size
                        if transfer_state.offset != transfer_state.total_size {
                            error!(
                                "Segmented download size mismatch. Expected {}, got {}",
                                transfer_state.total_size, transfer_state.offset
                            );
                            self.state = SdoServerState::Established; // Reset state even on error
                            return self.abort(command.header.transaction_id, 0x0607_0010);
                            // Data type mismatch / length
                        }

                        // Clone necessary data before resetting state
                        let index = transfer_state.index;
                        let sub_index = transfer_state.sub_index;
                        let data_buffer = transfer_state.data_buffer.clone(); // Clone data for OD write

                        // Finalize the write to OD
                        let result = self.write_to_od(index, sub_index, &data_buffer, od);
                        self.state = SdoServerState::Established; // Return to established state

                        match result {
                            Ok(_) => SdoCommand {
                                // Send final ACK
                                header: response_header,
                                data_size: None,
                                payload: Vec::new(),
                            },
                            Err(abort_code) => {
                                self.abort(command.header.transaction_id, abort_code)
                            }
                        }
                    } else {
                        // Acknowledge the intermediate segment.
                        SdoCommand {
                            header: response_header,
                            data_size: None,
                            payload: Vec::new(),
                        }
                    }
                } else {
                    // Received segment when not in SegmentedDownload state
                    error!("Received unexpected SDO segment frame.");
                    self.abort(command.header.transaction_id, 0x0504_0003) // Invalid sequence number (state implies sequence error)
                }
            }
        }
    }

    /// Helper to perform the final write to the Object Dictionary after all data is received.
    fn write_to_od(
        &self,
        index: u16,
        sub_index: u8,
        data: &[u8],
        od: &mut ObjectDictionary,
    ) -> Result<(), u32> {
        info!(
            "Writing {} bytes to OD 0x{:04X}/{}",
            data.len(),
            index,
            sub_index
        );
        match od.read(index, sub_index) {
            Some(type_template) => match ObjectValue::deserialize(data, &type_template) {
                Ok(value) => match od.write(index, sub_index, value) {
                    Ok(_) => Ok(()),
                    // Map OD write errors (which use PowerlinkError) to SDO Abort Codes
                    Err(PowerlinkError::StorageError("Object is read-only")) => Err(0x0601_0002), // Attempt to write read-only
                    Err(_) => Err(0x0800_0020), // Data cannot be transferred or stored
                },
                Err(_) => Err(0x0607_0010), // Data type mismatch or length error during deserialize
            },
            None => Err(0x0602_0000), // Object does not exist
        }
    }

    /// Creates an SDO Abort command.
    fn abort(&mut self, transaction_id: u8, abort_code: u32) -> SdoCommand {
        error!(
            "Aborting SDO transaction {}, code: {:#010X}",
            transaction_id, abort_code
        );
        // Reset state on abort
        self.state = SdoServerState::Established;
        SdoCommand {
            header: CommandLayerHeader {
                transaction_id,
                is_response: true,
                is_aborted: true,
                segmentation: Segmentation::Expedited,
                command_id: CommandId::Nil, // Command ID is arbitrary in aborts
                segment_size: 4,            // Size of the abort code payload
            },
            data_size: None,
            payload: abort_code.to_le_bytes().to_vec(),
        }
    }

    /// Updates the server state based on the incoming sequence layer header
    /// and determines the appropriate response header.
    fn process_sequence_layer(
        &mut self,
        request: SequenceLayerHeader,
    ) -> Result<SequenceLayerHeader, PowerlinkError> {
        debug!("Processing sequence layer in state {:?}", self.state);
        let mut response = SequenceLayerHeader::default(); // Start with default response

        match &mut self.state {
            SdoServerState::Closed => {
                if request.send_con == SendConnState::Initialization {
                    self.state = SdoServerState::Opening;
                    // Initialize sequence numbers based on client's first message
                    self.send_sequence_number = 0; // Server starts sending with 0 after init ACK
                    self.last_received_sequence_number = request.send_sequence_number;
                    info!(
                        "SDO connection initializing (Client Seq: {}).",
                        request.send_sequence_number
                    );
                    response.receive_con = ReceiveConnState::Initialization;
                    response.receive_sequence_number = self.last_received_sequence_number;
                    response.send_con = SendConnState::Initialization;
                    response.send_sequence_number = self.send_sequence_number; // Send 0
                } else {
                    warn!("Ignoring non-init request on closed SDO connection.");
                    // Send NoConnection response without changing state or seq numbers
                    // Do not return Err here, just send a NoConnection response
                    response.receive_con = ReceiveConnState::NoConnection;
                    response.send_con = SendConnState::NoConnection;
                    response.receive_sequence_number = request.send_sequence_number;
                    response.send_sequence_number = self.send_sequence_number;
                    // We return Ok, but the command layer processing will likely fail or do nothing
                }
            }
            SdoServerState::Opening => {
                // Client confirms connection with ConnectionValid and ACKs our Initialization seq num
                if request.send_con == SendConnState::ConnectionValid
                    && request.receive_sequence_number == self.send_sequence_number
                {
                    self.state = SdoServerState::Established;
                    self.last_received_sequence_number = request.send_sequence_number;
                    // Server now increments its send sequence number for the first real command response
                    self.send_sequence_number = self.send_sequence_number.wrapping_add(1) % 64;
                    info!(
                        "SDO connection established (Client Seq: {}). Ready for commands.",
                        request.send_sequence_number
                    );
                    response.receive_con = ReceiveConnState::ConnectionValid;
                    response.receive_sequence_number = self.last_received_sequence_number;
                    response.send_con = SendConnState::ConnectionValid;
                    response.send_sequence_number = self.send_sequence_number;
                } else {
                    error!(
                        "Invalid sequence state during SDO opening. Client Seq: {}, Client ACK: {}, Expected ACK: {}",
                        request.send_sequence_number,
                        request.receive_sequence_number,
                        self.send_sequence_number
                    );
                    self.state = SdoServerState::Closed; // Reset state on error
                    return Err(PowerlinkError::SdoSequenceError(
                        "Invalid sequence state during SDO opening.",
                    ));
                }
            }
            SdoServerState::Established | SdoServerState::SegmentedDownload(_) | SdoServerState::SegmentedUpload(_) => {
                // --- Sequence Number Check ---
                // Expected sequence number from the client
                let expected_seq = self.last_received_sequence_number.wrapping_add(1) % 64;

                // Handle retransmission request from client
                if request.receive_con == ReceiveConnState::ErrorResponse {
                    warn!(
                        "Client requested retransmission from sequence {}",
                        request.receive_sequence_number
                    );
                    // TODO: Implement retransmission logic from history buffer
                    // For now, just respond normally, acknowledging their request seq number
                    self.last_received_sequence_number = request.send_sequence_number;
                // Do NOT increment send_sequence_number here, as we'd resend the previous frame
                }
                // Handle duplicate frame from client
                else if request.send_sequence_number == self.last_received_sequence_number {
                    debug!(
                        "Duplicate SDO frame received (Seq: {}). Ignoring command, sending ACK.",
                        request.send_sequence_number
                    );
                    // Just send ACK, don't process command layer again
                    response.receive_con = ReceiveConnState::ConnectionValid;
                    response.receive_sequence_number = self.last_received_sequence_number;
                    response.send_con = SendConnState::ConnectionValid;
                    // Use the *same* send sequence number as the previous response
                    response.send_sequence_number = self.send_sequence_number;
                // Return immediately, skipping command processing
                // We must return Ok here, but signal to the caller to *not* process command
                // This is tricky. Let's let the command layer handle the empty payload.
                }
                // Handle out-of-order/lost frame from client
                else if request.send_sequence_number != expected_seq {
                    error!(
                        "SDO sequence number mismatch. Expected {}, got {}. Requesting retransmission from client.",
                        expected_seq, request.send_sequence_number
                    );
                    // Request retransmission from the client starting after the last good one.
                    // Do not update server state or sequence numbers on error
                    // Send ErrorResponse
                    response.receive_con = ReceiveConnState::ErrorResponse;
                    response.receive_sequence_number = self.last_received_sequence_number;
                    response.send_con = SendConnState::ConnectionValid;
                    response.send_sequence_number = self.send_sequence_number; // Resend our last frame's number
                    return Err(PowerlinkError::SdoSequenceError(
                        "SDO sequence number mismatch.",
                    )); // Signal error upstream
                }
                // --- Sequence OK ---
                else {
                    self.last_received_sequence_number = request.send_sequence_number;
                    // Acknowledgment received for an upload, so clear the timeout.
                    if let SdoServerState::SegmentedUpload(ref mut state) = self.state {
                        state.deadline_us = None;
                        state.last_sent_segment = None;
                    }

                    // Increment send sequence number *only if* we aren't handling retransmission/duplicates
                    if request.receive_con != ReceiveConnState::ErrorResponse {
                        self.send_sequence_number = self.send_sequence_number.wrapping_add(1) % 64;
                    }
                }

                // Default response for valid sequence
                response.receive_con = ReceiveConnState::ConnectionValid;
                response.receive_sequence_number = self.last_received_sequence_number;
                response.send_con = SendConnState::ConnectionValid; // Default, might be overridden by command layer
                response.send_sequence_number = self.send_sequence_number;
            }
        }
        Ok(response) // Return the calculated response header
    }

    /// Gets the next sequence number the server will use for sending.
    pub fn next_send_sequence(&self) -> u8 {
        self.send_sequence_number
    }

    /// Gets the last sequence number the server correctly received.
    pub fn current_receive_sequence(&self) -> u8 {
        self.last_received_sequence_number
    }
}

impl Default for SdoServer {
    fn default() -> Self {
        Self {
            state: SdoServerState::Closed,
            send_sequence_number: 0,
            last_received_sequence_number: 63, // Set to 63 (equiv to -1) so first received seq (0) is valid
            pending_client_requests: Vec::new(),
        }
    }
}