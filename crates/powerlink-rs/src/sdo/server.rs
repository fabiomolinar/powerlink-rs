// crates/powerlink-rs/src/sdo/server.rs
use crate::PowerlinkError;
use crate::frame::PRFlag;
use crate::od::{ObjectDictionary, ObjectValue};
use crate::sdo::command::{
    CommandId, CommandLayerHeader, DefaultSdoHandler, ReadByIndexRequest, ReadByNameRequest,
    ReadMultipleParamRequest, SdoCommand, SdoCommandHandler, Segmentation, WriteByIndexRequest,
};
use crate::sdo::sequence::{ReceiveConnState, SendConnState, SequenceLayerHeader};
use crate::sdo::state::{SdoServerState, SdoTransferState};
use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use log::{debug, error, trace, warn, info};

/// Manages a single SDO server connection.
pub struct SdoServer {
    state: SdoServerState,
    pub(super) send_sequence_number: u8,
    pub(super) last_received_sequence_number: u8,
    pub(super) pending_client_requests: Vec<Vec<u8>>,
    /// Optional handler for vendor-specific or complex commands.
    handler: Box<dyn SdoCommandHandler>,
}


const MAX_EXPEDITED_PAYLOAD: usize = 1452;
const OD_IDX_SDO_TIMEOUT: u16 = 0x1300;
const OD_IDX_SDO_RETRIES: u16 = 0x1302;

impl SdoServer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a new SdoServer with a custom command handler.
    pub fn with_handler<H: SdoCommandHandler + 'static>(handler: H) -> Self {
        Self {
            handler: Box::new(handler),
            ..Default::default()
        }
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
        match &self.state {
            SdoServerState::SegmentedUpload(state) => state.deadline_us,
            SdoServerState::SegmentedDownload(state) => state.deadline_us,
            _ => None,
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

        match &mut self.state {
            SdoServerState::SegmentedUpload(state) => {
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
            SdoServerState::SegmentedDownload(state) => {
                if let Some(deadline) = state.deadline_us {
                    if current_time_us >= deadline {
                        // Download timeout occurred, no retransmission possible, just abort.
                        error!("[SDO] Server: Segmented download timed out for TID {}. Aborting connection.", state.transaction_id);
                        abort_params = Some((state.transaction_id, 0x0504_0000)); // SDO protocol timed out
                    }
                }
            }
            _ => {} // No time-based logic for other states
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
        if request_sdo_payload.len() < 4 {
            return Err(PowerlinkError::BufferTooShort);
        }
        trace!("Handling SDO request payload: {:?}", request_sdo_payload);
        let sequence_header = SequenceLayerHeader::deserialize(&request_sdo_payload[0..4])?;
        let command_payload = &request_sdo_payload[4..];

        // --- Handle client retransmission request before sequence number processing ---
        if sequence_header.receive_con == ReceiveConnState::ErrorResponse {
            if let SdoServerState::SegmentedUpload(state) = &self.state {
                if let Some(last_command) = &state.last_sent_segment {
                    warn!("[SDO] Server: Client requested retransmission for TID {}. Resending last segment.", state.transaction_id);
                    let retransmit_header = SequenceLayerHeader {
                        receive_sequence_number: sequence_header.send_sequence_number, // Ack the request
                        receive_con: ReceiveConnState::ConnectionValid,
                        send_sequence_number: self.send_sequence_number, // Resend with same seq number
                        send_con: SendConnState::ConnectionValidAckRequest,
                    };
                    return self.serialize_sdo_response(retransmit_header, last_command.clone());
                }
            }
        }

        debug!("Parsed SDO sequence header: {:?}", sequence_header);
        let mut response_header = self.process_sequence_layer(sequence_header)?;

        if command_payload.is_empty() && (self.state == SdoServerState::Opening || sequence_header.send_con == SendConnState::ConnectionValid) {
            debug!("Received ACK or NIL command.");
            if let SdoServerState::SegmentedUpload(state) = core::mem::take(&mut self.state) {
                debug!("Client ACK received, continuing segmented upload.");
                let response_command = self.handle_segmented_upload(state, od, current_time_us);
                response_header.receive_sequence_number = self.last_received_sequence_number;
                return self.serialize_sdo_response(response_header, response_command);
            }

            let response_command = SdoCommand {
                header: CommandLayerHeader {
                    transaction_id: 0,
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
            return Err(PowerlinkError::InvalidPlFrame);
        }

        let sdo_command = SdoCommand::deserialize(command_payload)?;
        debug!("Parsed SDO command: {:?}", sdo_command);

        let response_command = self.process_command_layer(sdo_command, od, current_time_us);
        debug!("Generated SDO response command: {:?}", response_command);

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
                return self.handle_segmented_upload(state, od, current_time_us);
            }
            error!(
                "Mismatched transaction ID during segmented upload. Expected {}, got {}",
                state.transaction_id, command.header.transaction_id
            );
            self.state = SdoServerState::SegmentedUpload(state);
            return self.abort(command.header.transaction_id, 0x0800_0000);
        } else {
            self.state = current_state;
        }

        let response_header = CommandLayerHeader {
            transaction_id: command.header.transaction_id,
            is_response: true,
            ..Default::default()
        };

        match command.header.command_id {
            CommandId::ReadByIndex => self.handle_read_by_index(command, response_header, od, current_time_us),
            CommandId::WriteByIndex => self.handle_write_by_index(command, response_header, od, current_time_us),
            CommandId::ReadByName => self.handle_read_by_name(command, response_header, od, current_time_us),
            CommandId::WriteByName => self.handle_write_by_name(command, response_header, od),
            CommandId::ReadAllByIndex => self.handle_read_all_by_index(command, response_header, od, current_time_us),
            CommandId::ReadMultipleParamByIndex => self.handle_read_multiple_params(command, response_header, od, current_time_us),
            CommandId::MaxSegmentSize => self.handle_max_segment_size(command, response_header),
            CommandId::WriteAllByIndex => self.handler.handle_write_all_by_index(command, od),
            CommandId::WriteMultipleParamByIndex => self.handler.handle_write_multiple_params(command, od),
            CommandId::FileRead => self.handler.handle_file_read(command, od),
            CommandId::FileWrite => self.handler.handle_file_write(command, od),
            CommandId::Nil => {
                debug!("Received NIL command, sending empty ACK.");
                SdoCommand { header: response_header, data_size: None, payload: Vec::new() }
            }
        }
    }


    fn handle_read_by_index(
        &mut self,
        command: SdoCommand,
        mut response_header: CommandLayerHeader,
        od: &ObjectDictionary,
        current_time_us: u64,
    ) -> SdoCommand {
        match ReadByIndexRequest::from_payload(&command.payload) {
            Ok(req) => {
                info!("Processing SDO Read request for 0x{:04X}/{}", req.index, req.sub_index);
                match od.read(req.index, req.sub_index) {
                    Some(value) => {
                        let payload = value.serialize();
                        if payload.len() <= MAX_EXPEDITED_PAYLOAD {
                            info!("Responding with expedited read of {} bytes.", payload.len());
                            response_header.segment_size = payload.len() as u16;
                            SdoCommand { header: response_header, data_size: None, payload }
                        } else {
                            info!("Initiating segmented upload of {} bytes.", payload.len());
                            let transfer_state = SdoTransferState {
                                transaction_id: command.header.transaction_id,
                                total_size: payload.len(),
                                data_buffer: payload,
                                offset: 0,
                                index: req.index,
                                sub_index: req.sub_index,
                                deadline_us: None,
                                retransmissions_left: 0,
                                last_sent_segment: None,
                            };
                            self.handle_segmented_upload(transfer_state, od, current_time_us)
                        }
                    }
                    None => self.abort(command.header.transaction_id, 0x0602_0000), // Object does not exist
                }
            }
            Err(_) => self.abort(command.header.transaction_id, 0x0800_0000),
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
        current_time_us: u64,
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
                        let timeout_ms = od.read_u32(OD_IDX_SDO_TIMEOUT, 0).unwrap_or(15000) as u64;
                        self.state = SdoServerState::SegmentedDownload(SdoTransferState {
                            transaction_id: command.header.transaction_id,
                            total_size: command.data_size.unwrap_or(0) as usize,
                            data_buffer: req.data.to_vec(), // Store first segment
                            offset: req.data.len(),         // Track bytes received
                            index: req.index,
                            sub_index: req.sub_index,
                            deadline_us: Some(current_time_us + timeout_ms * 1000),
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
                if let SdoServerState::SegmentedDownload(ref mut transfer_state) = self.state {
                    if transfer_state.transaction_id != command.header.transaction_id {
                        error!(
                            "Mismatched transaction ID during segmented download. Expected {}, got {}",
                            transfer_state.transaction_id, command.header.transaction_id
                        );
                        return self.abort(command.header.transaction_id, 0x0800_0000);
                    }
                    transfer_state.data_buffer.extend_from_slice(&command.payload);
                    transfer_state.offset += command.payload.len();
                    debug!("Received download segment: new offset={}", transfer_state.offset);
                    if command.header.segmentation == Segmentation::Complete {
                        info!("Segmented download complete, writing to OD.");
                        if transfer_state.offset != transfer_state.total_size {
                            error!(
                                "Segmented download size mismatch. Expected {}, got {}",
                                transfer_state.total_size, transfer_state.offset
                            );
                            self.state = SdoServerState::Established;
                            return self.abort(command.header.transaction_id, 0x0607_0010);
                        }
                        let index = transfer_state.index;
                        let sub_index = transfer_state.sub_index;
                        let data_buffer = transfer_state.data_buffer.clone();
                        let result = self.write_to_od(index, sub_index, &data_buffer, od);
                        self.state = SdoServerState::Established;
                        match result {
                            Ok(_) => SdoCommand {
                                header: response_header,
                                data_size: None,
                                payload: Vec::new(),
                            },
                            Err(abort_code) => {
                                self.abort(command.header.transaction_id, abort_code)
                            }
                        }
                    } else {
                        // Reset the timeout for the next segment
                        let timeout_ms = od.read_u32(OD_IDX_SDO_TIMEOUT, 0).unwrap_or(15000) as u64;
                        transfer_state.deadline_us = Some(current_time_us + timeout_ms * 1000);
                        SdoCommand {
                            header: response_header,
                            data_size: None,
                            payload: Vec::new(),
                        }
                    }
                } else {
                    error!("Received unexpected SDO segment frame.");
                    self.abort(command.header.transaction_id, 0x0504_0003)
                }
            }
        }
    }

    fn handle_read_by_name(
        &mut self,
        command: SdoCommand,
        response_header: CommandLayerHeader,
        od: &ObjectDictionary,
        current_time_us: u64,
    ) -> SdoCommand {
        match ReadByNameRequest::from_payload(&command.payload) {
            Ok(req) => {
                info!("Processing SDO ReadByName request for '{}'", req.name);
                if let Some((index, sub_index)) = od.find_by_name(&req.name) {
                    // Found it, now treat it as a ReadByIndex
                    let read_req_command = SdoCommand {
                        payload: [index.to_le_bytes().as_slice(), &[sub_index]].concat(),
                        ..command
                    };
                    self.handle_read_by_index(read_req_command, response_header, od, current_time_us)
                } else {
                    self.abort(command.header.transaction_id, 0x060A_0023) // Resource not available
                }
            }
            Err(_) => self.abort(command.header.transaction_id, 0x0800_0000),
        }
    }

    fn handle_write_by_name(
        &mut self,
        command: SdoCommand,
        _response_header: CommandLayerHeader,
        _od: &mut ObjectDictionary,
    ) -> SdoCommand {
        // This is complex as payload contains name and data. Not implementing segmented transfer for it yet.
        warn!("WriteByName is not fully supported.");
        self.abort(command.header.transaction_id, 0x0601_0001) // Unsupported access
    }

    fn handle_read_all_by_index(
        &mut self,
        command: SdoCommand,
        mut response_header: CommandLayerHeader,
        od: &ObjectDictionary,
        current_time_us: u64,
    ) -> SdoCommand {
        match ReadByIndexRequest::from_payload(&command.payload) {
            Ok(req) if req.sub_index == 0 => {
                info!("Processing SDO ReadAllByIndex for 0x{:04X}", req.index);
                if let Some(crate::od::Object::Record(sub_indices)) = od.read_object(req.index) {
                    let mut payload = Vec::new();
                    for value in sub_indices {
                        payload.extend_from_slice(&value.serialize());
                    }
                    // Now send this payload, either expedited or segmented
                    if payload.len() <= MAX_EXPEDITED_PAYLOAD {
                        response_header.segment_size = payload.len() as u16;
                        SdoCommand { header: response_header, data_size: None, payload }
                    } else {
                        let transfer_state = SdoTransferState {
                            transaction_id: command.header.transaction_id,
                            total_size: payload.len(),
                            data_buffer: payload,
                            offset: 0,
                            index: req.index,
                            sub_index: 0,
                            deadline_us: None,
                            retransmissions_left: 0,
                            last_sent_segment: None,
                        };
                        self.handle_segmented_upload(transfer_state, od, current_time_us)
                    }
                } else {
                    self.abort(command.header.transaction_id, 0x0609_0030) // Value range exceeded (not a record)
                }
            }
            _ => self.abort(command.header.transaction_id, 0x0609_0011), // Sub-index does not exist
        }
    }

    fn handle_read_multiple_params(
        &mut self,
        command: SdoCommand,
        mut response_header: CommandLayerHeader,
        od: &ObjectDictionary,
        current_time_us: u64,
    ) -> SdoCommand {
        match ReadMultipleParamRequest::from_payload(&command.payload) {
            Ok(req) => {
                info!("Processing SDO ReadMultipleParamByIndex for {} entries", req.entries.len());
                let mut payload = Vec::new();
                for entry in req.entries {
                    match od.read(entry.index, entry.sub_index) {
                        Some(value) => {
                            let data = value.serialize();
                            payload.extend_from_slice(&(data.len() as u32).to_le_bytes());
                            payload.extend_from_slice(&data);
                        }
                        None => return self.abort(command.header.transaction_id, 0x0602_0000), // Object does not exist
                    }
                }
                // Now send this payload, either expedited or segmented
                if payload.len() <= MAX_EXPEDITED_PAYLOAD {
                    response_header.segment_size = payload.len() as u16;
                    SdoCommand { header: response_header, data_size: None, payload }
                } else {
                    let transfer_state = SdoTransferState {
                        transaction_id: command.header.transaction_id,
                        total_size: payload.len(),
                        data_buffer: payload,
                        offset: 0,
                        index: 0, // Not applicable
                        sub_index: 0, // Not applicable
                        deadline_us: None,
                        retransmissions_left: 0,
                        last_sent_segment: None,
                    };
                    self.handle_segmented_upload(transfer_state, od, current_time_us)
                }
            }
            Err(_) => self.abort(command.header.transaction_id, 0x0800_0000),
        }
    }

    fn handle_max_segment_size(
        &mut self,
        command: SdoCommand,
        mut response_header: CommandLayerHeader,
    ) -> SdoCommand {
        info!("Processing SDO MaxSegmentSize command");
        // We respond with our maximum supported size for a single SDO segment payload.
        let max_size = MAX_EXPEDITED_PAYLOAD as u32;
        response_header.segment_size = 4;
        SdoCommand {
            header: response_header,
            data_size: None,
            payload: max_size.to_le_bytes().to_vec(),
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

                // Handle retransmission request from client is handled in `handle_request`
                // Handle duplicate frame from client
                if request.send_sequence_number == self.last_received_sequence_number {
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

                    // Increment send sequence number
                    self.send_sequence_number = self.send_sequence_number.wrapping_add(1) % 64;
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
            handler: Box::new(DefaultSdoHandler),
        }
    }
}