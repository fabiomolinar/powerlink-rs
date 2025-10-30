// crates/powerlink-rs/src/sdo/server.rs
use crate::frame::basic::MacAddress;
use crate::frame::PRFlag;
use crate::od::ObjectValue;
use crate::sdo::command::{
    CommandId, CommandLayerHeader, DefaultSdoHandler, ReadByIndexRequest, ReadByNameRequest,
    ReadMultipleParamRequest, SdoCommand, SdoCommandHandler, Segmentation, WriteByIndexRequest,
    WriteByNameRequest,
};
use crate::sdo::sequence::{ReceiveConnState, SendConnState, SequenceLayerHeader};
use crate::sdo::sequence_handler::SdoSequenceHandler;
use crate::sdo::state::{SdoServerState, SdoTransferState};
#[cfg(feature = "sdo-udp")]
use crate::types::IpAddress;
use crate::{od::ObjectDictionary, PowerlinkError};
use alloc::boxed::Box;
use alloc::vec::Vec;
use log::{debug, error, info, trace, warn};

/// Holds transport-specific information about the SDO client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SdoClientInfo {
    /// SDO over ASnd (Layer 2)
    Asnd {
        source_node_id: crate::types::NodeId,
        source_mac: MacAddress,
    },
    /// SDO over UDP/IP (Layer 3/4)
    #[cfg(feature = "sdo-udp")]
    Udp {
        source_ip: IpAddress,
        source_port: u16,
    },
}

/// Manages a single SDO server connection.
/// This server stores the info of the *current* client
/// to handle stateful, multi-frame transfers and timeouts.
pub struct SdoServer {
    sequence_handler: SdoSequenceHandler,
    /// Optional handler for vendor-specific or complex commands.
    handler: Box<dyn SdoCommandHandler>,
    /// Information about the current or last connected client. Needed for server-initiated aborts.
    current_client_info: Option<SdoClientInfo>,
}

const MAX_EXPEDITED_PAYLOAD: usize = 1452; // Max SDO payload within ASnd or UDP
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

    /// Returns the absolute timestamp of the next SDO timeout, if any.
    pub fn next_action_time(&self) -> Option<u64> {
        match self.sequence_handler.state() {
            SdoServerState::SegmentedUpload(state) => state.deadline_us,
            SdoServerState::SegmentedDownload(state) => state.deadline_us,
            _ => None,
        }
    }

    /// Handles time-based events for the SDO server, like retransmission timeouts.
    /// Returns a full response tuple if an abort or retransmission frame
    /// needs to be sent.
    pub fn tick(
        &mut self,
        current_time_us: u64,
        od: &ObjectDictionary,
    ) -> Result<Option<(SdoClientInfo, SequenceLayerHeader, SdoCommand)>, PowerlinkError> {
        let mut retransmit_command = None;
        let mut abort_params: Option<(u8, u32)> = None;

        match self.sequence_handler.state_mut() {
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
                            let timeout_ms =
                                od.read_u32(OD_IDX_SDO_TIMEOUT, 0).unwrap_or(15000) as u64;
                            state.deadline_us = Some(current_time_us + timeout_ms * 1000);

                            // Retransmit the last sent segment.
                            if let Some(last_command) = &state.last_sent_segment {
                                retransmit_command = Some(last_command.clone());
                            } else {
                                // This should not happen if we are in this state.
                                return Err(PowerlinkError::InternalError(
                                    "Missing last sent segment during retransmission",
                                ));
                            }
                        } else {
                            // No retransmissions left, abort the connection.
                            error!(
                                "[SDO] Server: No retransmissions left for TID {}. Aborting connection.",
                                state.transaction_id
                            );
                            abort_params = Some((state.transaction_id, 0x0504_0000)); // SDO protocol timed out
                        }
                    }
                }
            }
            SdoServerState::SegmentedDownload(state) => {
                if let Some(deadline) = state.deadline_us {
                    if current_time_us >= deadline {
                        // Download timeout occurred, no retransmission possible, just abort.
                        error!(
                            "[SDO] Server: Segmented download timed out for TID {}. Aborting connection.",
                            state.transaction_id
                        );
                        abort_params = Some((state.transaction_id, 0x0504_0000)); // SDO protocol timed out
                    }
                }
            }
            _ => {} // No time-based logic for other states
        }

        // Handle retransmission or abort triggered by timeout
        if let Some(command) = retransmit_command {
            let response_header = SequenceLayerHeader {
                receive_sequence_number: self.sequence_handler.current_receive_sequence(),
                receive_con: ReceiveConnState::ConnectionValid,
                send_sequence_number: self.sequence_handler.next_send_sequence(), // Use the same sequence number
                send_con: SendConnState::ConnectionValidAckRequest, // Request ACK again
            };
            // Need client info to construct SdoResponseData
            if let Some(client_info) = self.current_client_info {
                return Ok(Some((client_info, response_header, command)));
            } else {
                // Should not happen if a transfer was active
                return Err(PowerlinkError::InternalError(
                    "Missing client info during SDO timeout handling",
                ));
            }
        }

        if let Some((tid, code)) = abort_params {
            let abort_command = self.abort(tid, code);
            let response_header = SequenceLayerHeader {
                receive_sequence_number: self.sequence_handler.current_receive_sequence(),
                receive_con: ReceiveConnState::ConnectionValid,
                send_sequence_number: self.sequence_handler.next_send_sequence(),
                send_con: SendConnState::NoConnection, // Closing connection
            };
            // Need client info to construct SdoResponseData
            if let Some(client_info) = self.current_client_info {
                // Abort also clears current_client_info
                self.current_client_info = None;
                return Ok(Some((client_info, response_header, abort_command)));
            } else {
                // Should not happen if a transfer was active
                return Err(PowerlinkError::InternalError(
                    "Missing client info during SDO timeout handling",
                ));
            }
        }

        Ok(None) // No action from tick
    }

    /// Processes an incoming SDO request payload (starting *directly* with the Sequence Layer header).
    ///
    /// Returns a (SequenceLayerHeader, SdoCommand) tuple, which the caller is
    /// responsible for packaging into a transport-specific response.
    pub fn handle_request(
        &mut self,
        request_sdo_payload: &[u8], // Starts with Sequence Layer Header
        client_info: SdoClientInfo, // Pass transport info
        od: &mut ObjectDictionary,
        current_time_us: u64,
    ) -> Result<(SequenceLayerHeader, SdoCommand), PowerlinkError> {
        if request_sdo_payload.len() < 4 {
            return Err(PowerlinkError::BufferTooShort); // Need at least sequence header
        }
        trace!("Handling SDO request payload: {:?}", request_sdo_payload);

        // Store client info for this connection.
        // This is necessary for tick-based timeouts/aborts.
        self.current_client_info = Some(client_info);

        let sequence_header = SequenceLayerHeader::deserialize(&request_sdo_payload[0..4])?;
        let command_payload = &request_sdo_payload[4..]; // Rest is command layer + data

        // --- Handle client retransmission request before sequence number processing ---
        if sequence_header.receive_con == ReceiveConnState::ErrorResponse {
            if let SdoServerState::SegmentedUpload(state) = self.sequence_handler.state() {
                if let Some(last_command) = &state.last_sent_segment {
                    warn!(
                        "[SDO] Server: Client requested retransmission for TID {}. Resending last segment.",
                        state.transaction_id
                    );
                    let retransmit_header = SequenceLayerHeader {
                        receive_sequence_number: sequence_header.send_sequence_number, // Ack the request
                        receive_con: ReceiveConnState::ConnectionValid,
                        send_sequence_number: self.sequence_handler.next_send_sequence(), // Resend with same seq number
                        send_con: SendConnState::ConnectionValidAckRequest,
                    };
                    return Ok((retransmit_header, last_command.clone()));
                }
            }
            // Ignore ErrorResponse if not in segmented upload or no segment stored
            warn!("[SDO] Server: Received ErrorResponse in unexpected state or missing segment.");
            // Send a basic ACK without processing command
            let ack_header = self
                .sequence_handler
                .process_sequence_layer(sequence_header)?;
            let nil_command = SdoCommand {
                header: Default::default(),
                data_size: None,
                payload: Vec::new(),
            };
            return Ok((ack_header, nil_command));
        }

        debug!("Parsed SDO sequence header: {:?}", sequence_header);
        let mut response_header = self
            .sequence_handler
            .process_sequence_layer(sequence_header)?;

        // Handle ACK-only or NIL command frames (no command payload)
        if command_payload.is_empty()
            && (*self.sequence_handler.state() == SdoServerState::Opening
                || sequence_header.send_con == SendConnState::ConnectionValid)
        {
            debug!("Received ACK or NIL command.");
            if let SdoServerState::SegmentedUpload(mut state) =
                core::mem::take(self.sequence_handler.state_mut())
            {
                // Client ACK received, continue segmented upload.
                debug!("Client ACK received, continuing segmented upload.");
                let (response_command, is_last) =
                    state.get_next_upload_segment(od, current_time_us);
                
                // If not last, put state back. If last, transition to Established.
                if !is_last {
                    *self.sequence_handler.state_mut() = SdoServerState::SegmentedUpload(state);
                } else {
                    *self.sequence_handler.state_mut() = SdoServerState::Established;
                    // Last segment, connection is now idle, clear client info
                    if response_command.header.segmentation == Segmentation::Complete {
                        self.current_client_info = None;
                    }
                }

                response_header.receive_sequence_number =
                    self.sequence_handler.current_receive_sequence();
                return Ok((response_header, response_command));
            }
            // Just send back an ACK if Established and command payload is empty
            if self.sequence_handler.state() == &SdoServerState::Established {
                let response_command = SdoCommand {
                    header: CommandLayerHeader {
                        transaction_id: 0, // Transaction ID might not be known here
                        is_response: true,
                        ..Default::default()
                    },
                    data_size: None,
                    payload: Vec::new(),
                };
                response_header.receive_sequence_number =
                    self.sequence_handler.current_receive_sequence();
                // This is a simple ACK, not part of a larger transfer, clear client info
                self.current_client_info = None;
                return Ok((response_header, response_command));
            }
            // If Opening and empty payload, something is wrong, fall through to error
            error!("Received empty command payload during Opening state.");
            return Err(PowerlinkError::SdoInvalidCommandPayload); // Treat as error
        }

        if command_payload.is_empty() {
            error!("Received empty command payload in unexpected state.");
            return Err(PowerlinkError::SdoInvalidCommandPayload);
        }

        // Deserialize and process the command layer
        match SdoCommand::deserialize(command_payload) {
            Ok(sdo_command) => {
                debug!("Parsed SDO command: {:?}", sdo_command);
                let response_command = self.process_command_layer(sdo_command, od, current_time_us);
                debug!("Generated SDO response command: {:?}", response_command);

                response_header.receive_sequence_number =
                    self.sequence_handler.current_receive_sequence();
                
                // If this is the end of a transfer, clear client info
                if response_command.header.segmentation == Segmentation::Expedited
                    || response_command.header.segmentation == Segmentation::Complete
                    || response_command.header.is_aborted
                {
                    self.current_client_info = None;
                }
                if response_command.header.is_aborted {
                    response_header.send_con = SendConnState::NoConnection;
                }

                // Return the response data components
                Ok((response_header, response_command))
            }
            Err(e) => {
                // Failed to deserialize command - create an Abort response
                error!("Failed to deserialize SDO command payload: {:?}", e);
                // Try to get transaction ID from the raw payload if possible (best effort)
                let tid = command_payload.first().map_or(0, |flags| flags & 0x0F);
                let abort_command = self.abort(tid, 0x0504_0001); // Invalid command specifier
                response_header.receive_sequence_number =
                    self.sequence_handler.current_receive_sequence();
                // Abort implies connection closure at sequence layer for response
                response_header.send_con = SendConnState::NoConnection;
                // Abort clears client info
                self.current_client_info = None;
                Ok((response_header, abort_command))
            }
        }
    }

    /// Processes the SDO command, interacts with the OD, and returns a response command.
    fn process_command_layer(
        &mut self,
        command: SdoCommand,
        od: &mut ObjectDictionary,
        current_time_us: u64,
    ) -> SdoCommand {
        // Temporarily take ownership of the state to avoid borrow checker issues.
        let current_state = core::mem::take(self.sequence_handler.state_mut());

        // If we are in a segmented upload, any new valid command from the client
        // just serves as an ACK to trigger the next segment.
        if let SdoServerState::SegmentedUpload(mut state) = current_state {
            // Check if the received command's transaction ID matches the ongoing upload
            if state.transaction_id == command.header.transaction_id && !command.header.is_aborted {
                // Also check if the client is ACKing the segment we sent
                if command.header.command_id == CommandId::Nil || command.header.is_response {
                    // Check if it looks like an ACK
                    debug!(
                        "Client ACK received during segmented upload (TID {}). Sending next segment.",
                        state.transaction_id
                    );
                    let (response_command, is_last) =
                        state.get_next_upload_segment(od, current_time_us);
                    if !is_last {
                        *self.sequence_handler.state_mut() =
                            SdoServerState::SegmentedUpload(state);
                    } else {
                        *self.sequence_handler.state_mut() = SdoServerState::Established;
                    }
                    return response_command;
                } else {
                    // Received a new request command during upload - this is an error
                    error!(
                        "Received new request (CmdID: {:?}, TID: {}) during segmented upload (TID: {}). Aborting.",
                        command.header.command_id,
                        command.header.transaction_id,
                        state.transaction_id
                    );
                    *self.sequence_handler.state_mut() = SdoServerState::SegmentedUpload(state); // Put state back before aborting
                    return self.abort(command.header.transaction_id, 0x0504_0003); // Invalid sequence
                }
            } else if command.header.is_aborted {
                info!(
                    "Client aborted segmented upload (TID {}).",
                    command.header.transaction_id
                );
                // Client aborted, just transition back to Established
                *self.sequence_handler.state_mut() = SdoServerState::Established;
                // Don't send another abort back
                return SdoCommand {
                    header: Default::default(),
                    data_size: None,
                    payload: Vec::new(),
                }; // Empty response needed for sequence layer
            } else {
                error!(
                    "Mismatched transaction ID during segmented upload. Expected {}, got {}",
                    state.transaction_id, command.header.transaction_id
                );
                *self.sequence_handler.state_mut() = SdoServerState::SegmentedUpload(state); // Put state back
                return self.abort(command.header.transaction_id, 0x0800_0000); // General error
            }
        } else {
            // Not in segmented upload, put state back and process command normally
            *self.sequence_handler.state_mut() = current_state;
        }

        // Handle client abort received in Established or other states
        if command.header.is_aborted {
            info!(
                "Client aborted SDO transfer (TID {}).",
                command.header.transaction_id
            );
            *self.sequence_handler.state_mut() = SdoServerState::Established; // Ensure state is reset
            // Don't send another abort back
            return SdoCommand {
                header: Default::default(),
                data_size: None,
                payload: Vec::new(),
            }; // Empty response needed for sequence layer
        }

        let response_header = CommandLayerHeader {
            transaction_id: command.header.transaction_id,
            is_response: true,
            ..Default::default()
        };

        match command.header.command_id {
            CommandId::ReadByIndex => {
                self.handle_read_by_index(command, response_header, od, current_time_us)
            }
            CommandId::WriteByIndex => {
                self.handle_write_by_index(command, response_header, od, current_time_us)
            }
            CommandId::ReadByName => {
                self.handle_read_by_name(command, response_header, od, current_time_us)
            }
            CommandId::WriteByName => {
                self.handle_write_by_name(command, response_header, od, current_time_us)
            }
            CommandId::ReadAllByIndex => {
                self.handle_read_all_by_index(command, response_header, od, current_time_us)
            }
            CommandId::ReadMultipleParamByIndex => {
                self.handle_read_multiple_params(command, response_header, od, current_time_us)
            }
            CommandId::MaxSegmentSize => self.handle_max_segment_size(command, response_header),
            CommandId::WriteAllByIndex => self.handler.handle_write_all_by_index(command, od),
            CommandId::WriteMultipleParamByIndex => {
                self.handler.handle_write_multiple_params(command, od)
            }
            CommandId::FileRead => self.handler.handle_file_read(command, od),
            CommandId::FileWrite => self.handler.handle_file_write(command, od),
            CommandId::Nil => {
                debug!("Received NIL command, sending empty ACK.");
                SdoCommand {
                    header: response_header,
                    data_size: None,
                    payload: Vec::new(),
                }
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
                info!(
                    "Processing SDO Read request for 0x{:04X}/{}",
                    req.index, req.sub_index
                );
                match od.read(req.index, req.sub_index) {
                    Some(value) => {
                        let payload = value.serialize();
                        if payload.len() <= MAX_EXPEDITED_PAYLOAD {
                            info!("Responding with expedited read of {} bytes.", payload.len());
                            response_header.segment_size = payload.len() as u16;
                            SdoCommand {
                                header: response_header,
                                data_size: None,
                                payload,
                            }
                        } else {
                            info!("Initiating segmented upload of {} bytes.", payload.len());
                            let mut transfer_state = SdoTransferState {
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
                            // Get the first segment
                            let (response_command, is_last) =
                                transfer_state.get_next_upload_segment(od, current_time_us);

                            // Store state *if* not complete
                            if !is_last {
                                *self.sequence_handler.state_mut() =
                                    SdoServerState::SegmentedUpload(transfer_state);
                            }
                            // Return the first segment
                            response_command
                        }
                    }
                    // Map OD read errors (Object/SubObjectNotFound) to SDO Abort codes
                    None if od.read_object(req.index).is_none() => {
                        self.abort(command.header.transaction_id, 0x0602_0000) // Object does not exist
                    }
                    None => {
                        self.abort(command.header.transaction_id, 0x0609_0011) // Sub-index does not exist
                    }
                }
            }
            Err(PowerlinkError::SdoInvalidCommandPayload) => {
                self.abort(command.header.transaction_id, 0x0504_0001) // Command specifier invalid
            }
            Err(_) => {
                // Other parsing errors
                self.abort(command.header.transaction_id, 0x0800_0000) // General error
            }
        }
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
                info!(
                    "Processing expedited SDO Write (TID {}).",
                    command.header.transaction_id
                );
                // Handle a complete write in a single frame.
                match WriteByIndexRequest::from_payload(&command.payload) {
                    Ok(req) => {
                        // Create a temporary SdoTransferState to use perform_od_write
                        let state = SdoTransferState {
                            transaction_id: command.header.transaction_id,
                            total_size: req.data.len(),
                            data_buffer: req.data.to_vec(),
                            offset: req.data.len(),
                            index: req.index,
                            sub_index: req.sub_index,
                            deadline_us: None,
                            retransmissions_left: 0,
                            last_sent_segment: None,
                        };

                        match state.perform_od_write(od) {
                            Ok(_) => SdoCommand {
                                header: response_header,
                                data_size: None,
                                payload: Vec::new(), // Successful write has empty payload
                            },
                            Err(abort_code) => {
                                self.abort(command.header.transaction_id, abort_code)
                            }
                        }
                    }
                    Err(PowerlinkError::SdoInvalidCommandPayload) => {
                        self.abort(command.header.transaction_id, 0x0504_0001) // Command specifier invalid
                    }
                    Err(_) => self.abort(command.header.transaction_id, 0x0800_0000), // General error parsing payload
                }
            }
            Segmentation::Initiate => {
                info!(
                    "Initiating segmented SDO download (TID {}).",
                    command.header.transaction_id
                );
                // Start a new segmented download.
                match WriteByIndexRequest::from_payload(&command.payload) {
                    Ok(req) => {
                        // Check data_size consistency
                        let total_size = command.data_size.unwrap_or(0) as usize;
                        if total_size == 0 {
                            error!("Segmented Download Initiate received with DataSize=0.");
                            return self.abort(command.header.transaction_id, 0x0607_0010); // Type mismatch/length error
                        }
                        let timeout_ms = od.read_u32(OD_IDX_SDO_TIMEOUT, 0).unwrap_or(15000) as u64;
                        *self.sequence_handler.state_mut() =
                            SdoServerState::SegmentedDownload(SdoTransferState {
                                transaction_id: command.header.transaction_id,
                                total_size,
                                data_buffer: req.data.to_vec(), // Store first segment's data
                                offset: req.data.len(),         // Track bytes received
                                index: req.index,
                                sub_index: req.sub_index,
                                deadline_us: Some(current_time_us + timeout_ms * 1000),
                                retransmissions_left: 0, // Not applicable for server download
                                last_sent_segment: None, // Not applicable for server download
                            });
                        SdoCommand {
                            header: response_header, // Send ACK response
                            data_size: None,
                            payload: Vec::new(),
                        }
                    }
                    Err(PowerlinkError::SdoInvalidCommandPayload) => {
                        self.abort(command.header.transaction_id, 0x0504_0001) // Command specifier invalid
                    }
                    Err(_) => self.abort(command.header.transaction_id, 0x0800_0000), // General error parsing payload
                }
            }
            Segmentation::Segment | Segmentation::Complete => {
                // Take the state to avoid mutable borrow issues when calling write_to_od/abort
                if let SdoServerState::SegmentedDownload(mut transfer_state) =
                    core::mem::take(self.sequence_handler.state_mut())
                {
                    if transfer_state.transaction_id != command.header.transaction_id {
                        error!(
                            "Mismatched transaction ID during segmented download. Expected {}, got {}",
                            transfer_state.transaction_id, command.header.transaction_id
                        );
                        // Put state back before aborting
                        *self.sequence_handler.state_mut() =
                            SdoServerState::SegmentedDownload(transfer_state);
                        return self.abort(command.header.transaction_id, 0x0800_0000); // General error
                    }

                    // Delegate processing to the transfer state
                    match transfer_state.process_download_segment(&command, od, current_time_us) {
                        Ok(true) => {
                            // Complete and successful
                            *self.sequence_handler.state_mut() = SdoServerState::Established;
                            SdoCommand {
                                header: response_header, // Send final ACK
                                data_size: None,
                                payload: Vec::new(),
                            }
                        }
                        Ok(false) => {
                            // More segments needed
                            *self.sequence_handler.state_mut() =
                                SdoServerState::SegmentedDownload(transfer_state);
                            SdoCommand {
                                header: response_header, // Send ACK for segment
                                data_size: None,
                                payload: Vec::new(),
                            }
                        }
                        Err(abort_code) => {
                            // Abort
                            *self.sequence_handler.state_mut() = SdoServerState::Established;
                            self.abort(command.header.transaction_id, abort_code)
                        }
                    }
                } else {
                    error!(
                        "Received unexpected SDO segment frame (TID {}). Current state: {:?}",
                        command.header.transaction_id,
                        self.sequence_handler.state()
                    );
                    // Abort, reset state just in case
                    *self.sequence_handler.state_mut() = SdoServerState::Established;
                    self.abort(command.header.transaction_id, 0x0504_0003) // Invalid sequence
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
                        // Construct payload for ReadByIndex
                        payload: [index.to_le_bytes().as_slice(), &[sub_index], &[0u8]].concat(), // Index(2)+Sub(1)+Reserved(1)=4
                        header: CommandLayerHeader {
                            segment_size: 4,
                            ..command.header
                        }, // Update segment size
                        ..command
                    };
                    self.handle_read_by_index(
                        read_req_command,
                        response_header,
                        od,
                        current_time_us,
                    )
                } else {
                    self.abort(command.header.transaction_id, 0x060A_0023) // Resource not available
                }
            }
            Err(PowerlinkError::SdoInvalidCommandPayload) => {
                self.abort(command.header.transaction_id, 0x0504_0001) // Command specifier invalid
            }
            Err(_) => self.abort(command.header.transaction_id, 0x0800_0000), // General error
        }
    }

    fn handle_write_by_name(
        &mut self,
        mut command: SdoCommand,
        response_header: CommandLayerHeader,
        od: &mut ObjectDictionary,
        current_time_us: u64,
    ) -> SdoCommand {
        match WriteByNameRequest::from_payload(&command.payload) {
            Ok(req) => {
                info!("Processing SDO WriteByName for '{}'", req.name);
                if let Some((index, sub_index)) = od.find_by_name(&req.name) {
                    // Reconstruct the payload to match WriteByIndex format: [index(2), sub_index(1), reserved(1), data...]
                    let mut new_payload = Vec::with_capacity(4 + req.data.len());
                    new_payload.extend_from_slice(&index.to_le_bytes());
                    new_payload.push(sub_index);
                    new_payload.push(0u8); // Reserved byte
                    new_payload.extend_from_slice(req.data);
                    command.payload = new_payload;
                    command.header.segment_size = command.payload.len() as u16; // Update size

                    // Delegate to the existing WriteByIndex handler
                    self.handle_write_by_index(command, response_header, od, current_time_us)
                } else {
                    self.abort(command.header.transaction_id, 0x060A_0023) // Resource not available
                }
            }
            Err(PowerlinkError::SdoInvalidCommandPayload) => {
                self.abort(command.header.transaction_id, 0x0504_0001) // Command specifier invalid
            }
            Err(_) => self.abort(command.header.transaction_id, 0x0800_0000), // General error
        }
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
                // Need to handle different OD object types correctly
                match od.read_object(req.index) {
                    Some(crate::od::Object::Record(sub_indices))
                    | Some(crate::od::Object::Array(sub_indices)) => {
                        let mut payload = Vec::new();
                        // Iterate elements (sub-index 1 onwards)
                        for i in 0..sub_indices.len() {
                            // Read each sub-index individually
                            if let Some(value) = od.read(req.index, (i + 1) as u8) {
                                payload.extend_from_slice(&value.serialize());
                            } else {
                                // Should not happen if read_object succeeded and length is correct
                                warn!(
                                    "Failed to read sub-index {} during ReadAllByIndex for 0x{:04X}",
                                    i + 1,
                                    req.index
                                );
                                // Abort if a sub-index read fails? Or continue with partial data?
                                // Let's abort for consistency.
                                return self.abort(command.header.transaction_id, 0x0609_0011); // Sub-index access error
                            }
                        }
                        // Now send this payload, either expedited or segmented
                        if payload.len() <= MAX_EXPEDITED_PAYLOAD {
                            response_header.segment_size = payload.len() as u16;
                            SdoCommand {
                                header: response_header,
                                data_size: None,
                                payload,
                            }
                        } else {
                            let mut transfer_state = SdoTransferState {
                                transaction_id: command.header.transaction_id,
                                total_size: payload.len(),
                                data_buffer: payload,
                                offset: 0,
                                index: req.index,
                                sub_index: 0, // Signifies ReadAll
                                deadline_us: None,
                                retransmissions_left: 0,
                                last_sent_segment: None,
                            };
                            let (response_command, is_last) =
                                transfer_state.get_next_upload_segment(od, current_time_us);
                            if !is_last {
                                *self.sequence_handler.state_mut() =
                                    SdoServerState::SegmentedUpload(transfer_state);
                            }
                            response_command
                        }
                    }
                    Some(crate::od::Object::Variable(_)) => {
                        // ReadAllByIndex is not valid for Variables
                        self.abort(command.header.transaction_id, 0x0609_0030) // Value range exceeded (not a record/array)
                    }
                    None => {
                        // Object itself doesn't exist
                        self.abort(command.header.transaction_id, 0x0602_0000) // Object does not exist
                    }
                }
            }
            Ok(_) => {
                // Sub-index was not 0
                self.abort(command.header.transaction_id, 0x0609_0011) // Sub-index parameter invalid for ReadAll
            }
            Err(PowerlinkError::SdoInvalidCommandPayload) => {
                self.abort(command.header.transaction_id, 0x0504_0001) // Command specifier invalid
            }
            Err(_) => self.abort(command.header.transaction_id, 0x0800_0000), // General error
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
                info!(
                    "Processing SDO ReadMultipleParamByIndex for {} entries",
                    req.entries.len()
                );
                let mut payload = Vec::new();
                // Iterate using a reference to avoid moving req.entries
                for entry in &req.entries {
                    match od.read(entry.index, entry.sub_index) {
                        Some(value) => {
                            let data = value.serialize();
                            // Add Sub-Abort=0, reserved=0, Padding=0 before data length and data
                            payload.push(0u8); // SubAbort=0, reserved=0
                            payload.push(0u8); // Padding=0
                            payload.extend_from_slice(&(data.len() as u16).to_le_bytes()); // Use u16 for length field per spec
                            payload.extend_from_slice(&data);
                            // Ensure 4-byte alignment for the next entry
                            while payload.len() % 4 != 0 {
                                payload.push(0u8); // Padding byte
                            }
                        }
                        None => {
                            // If *any* entry is not found, abort the whole request
                            let abort_code = if od.read_object(entry.index).is_none() {
                                0x0602_0000
                            } else {
                                0x0609_0011
                            };
                            return self.abort(command.header.transaction_id, abort_code);
                        }
                    }
                }
                // Prepend total number of entries
                let mut final_payload = Vec::new();
                // Use req.entries.len() here, which is now valid
                final_payload.extend_from_slice(&(req.entries.len() as u32).to_le_bytes()); // Number of entries as U32
                final_payload.append(&mut payload);

                // Now send this final_payload, either expedited or segmented
                if final_payload.len() <= MAX_EXPEDITED_PAYLOAD {
                    response_header.segment_size = final_payload.len() as u16;
                    SdoCommand {
                        header: response_header,
                        data_size: None,
                        payload: final_payload,
                    }
                } else {
                    let mut transfer_state = SdoTransferState {
                        transaction_id: command.header.transaction_id,
                        total_size: final_payload.len(),
                        data_buffer: final_payload,
                        offset: 0,
                        index: 0,     // Not applicable
                        sub_index: 0, // Not applicable
                        deadline_us: None,
                        retransmissions_left: 0,
                        last_sent_segment: None,
                    };
                    let (response_command, is_last) =
                        transfer_state.get_next_upload_segment(od, current_time_us);
                    if !is_last {
                        *self.sequence_handler.state_mut() =
                            SdoServerState::SegmentedUpload(transfer_state);
                    }
                    response_command
                }
            }
            Err(PowerlinkError::SdoInvalidCommandPayload) => {
                self.abort(command.header.transaction_id, 0x0504_0001) // Command specifier invalid
            }
            Err(_) => self.abort(command.header.transaction_id, 0x0800_0000), // General error
        }
    }

    fn handle_max_segment_size(
        &mut self,
        command: SdoCommand,
        mut response_header: CommandLayerHeader,
    ) -> SdoCommand {
        info!("Processing SDO MaxSegmentSize command");
        // We respond with our maximum supported size for a single SDO segment payload.
        let max_size_server = MAX_EXPEDITED_PAYLOAD as u16; // Our server capability
        // Extract client's max size from payload
        let max_size_client = if command.payload.len() >= 2 {
            u16::from_le_bytes(command.payload[0..2].try_into().unwrap_or([0, 0]))
        } else {
            0 // Invalid request payload
        };

        response_header.segment_size = 4; // Response contains MSS Client + MSS Server (2+2 bytes)
        let response_payload =
            [max_size_client.to_le_bytes(), max_size_server.to_le_bytes()].concat();

        SdoCommand {
            header: response_header,
            data_size: None,
            payload: response_payload,
        }
    }

    /// Creates an SDO Abort command. Resets internal state.
    fn abort(&mut self, transaction_id: u8, abort_code: u32) -> SdoCommand {
        error!(
            "Aborting SDO transaction {}, code: {:#010X}",
            transaction_id, abort_code
        );
        // Reset state on abort
        self.sequence_handler.reset(); // Go back to Established, not Closed
        self.current_client_info = None; // Clear client info on abort
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

    /// Gets the next sequence number the server will use for sending.
    pub fn next_send_sequence(&self) -> u8 {
        self.sequence_handler.next_send_sequence()
    }

    /// Gets the last sequence number the server correctly received.
    pub fn current_receive_sequence(&self) -> u8 {
        self.sequence_handler.current_receive_sequence()
    }
}

impl Default for SdoServer {
    fn default() -> Self {
        Self {
            sequence_handler: SdoSequenceHandler::new(),
            handler: Box::new(DefaultSdoHandler),
            current_client_info: None,
        }
    }
}

