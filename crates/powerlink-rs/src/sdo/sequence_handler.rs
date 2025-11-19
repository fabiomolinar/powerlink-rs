// crates/powerlink-rs/src/sdo/sequence_handler.rs
use crate::hal::PowerlinkError;
use crate::od::ObjectDictionary;
use crate::sdo::OD_IDX_SDO_TIMEOUT;
use crate::sdo::command::{
    CommandId, CommandLayerHeader, SdoCommand, SdoCommandHandler, Segmentation,
};
use crate::sdo::handlers;
use crate::sdo::sequence::{ReceiveConnState, SendConnState, SequenceLayerHeader};
use crate::sdo::server::SdoClientInfo;
use crate::sdo::state::SdoServerState;
use crate::sdo::transport::SdoResponseData;
use alloc::vec::Vec;
use core::mem;
use log::{debug, error, info, trace, warn};

/// Manages the SDO Sequence Layer logic, including connection state and sequence numbers.
#[derive(Debug)]
pub struct SdoSequenceHandler {
    state: SdoServerState,
    send_sequence_number: u8,
    last_received_sequence_number: u8,
    /// Information about the client this handler is managing.
    client_info: SdoClientInfo,
}

impl SdoSequenceHandler {
    pub fn new(client_info: SdoClientInfo) -> Self {
        Self {
            state: SdoServerState::Closed, // Initial state, will transition on first request
            send_sequence_number: 0,
            last_received_sequence_number: 63, // Set to 63 (equiv to -1) so first received seq (0) is valid
            client_info,
        }
    }

    /// Returns a reference to the current SDO server state.
    pub fn state(&self) -> &SdoServerState {
        &self.state
    }

    /// Returns a mutable reference to the current SDO server state.
    pub fn state_mut(&mut self) -> &mut SdoServerState {
        &mut self.state
    }

    /// Returns true if the connection is closed and can be pruned.
    pub fn is_closed(&self) -> bool {
        matches!(self.state, SdoServerState::Closed)
    }

    /// Resets the connection state to Closed.
    pub fn reset(&mut self) {
        self.state = SdoServerState::Closed;
        self.send_sequence_number = 0;
        self.last_received_sequence_number = 63;
    }

    /// Increments the send sequence number, wrapping at 64.
    pub fn increment_send_seq(&mut self) {
        self.send_sequence_number = self.send_sequence_number.wrapping_add(1) % 64;
    }

    /// Gets the next sequence number the server will use for sending.
    pub fn next_send_sequence(&self) -> u8 {
        self.send_sequence_number
    }

    /// Gets the last sequence number the server correctly received.
    pub fn current_receive_sequence(&self) -> u8 {
        self.last_received_sequence_number
    }

    /// Creates an SDO Abort command. Resets internal state to Closed.
    pub(super) fn abort(&mut self, transaction_id: u8, abort_code: u32) -> SdoCommand {
        error!(
            "Aborting SDO transaction {}, code: {:#010X}",
            transaction_id, abort_code
        );
        // Reset state to Closed on abort to signal for pruning
        self.reset();
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

    /// Handles time-based events for the SDO server, like retransmission timeouts.
    /// Returns a full response tuple if an abort or retransmission frame
    /// needs to be sent.
    pub fn tick(
        &mut self,
        current_time_us: u64,
        od: &ObjectDictionary,
    ) -> Result<Option<SdoResponseData>, PowerlinkError> {
        let mut retransmit_command = None;
        let mut abort_params: Option<(u8, u32)> = None;

        match self.state_mut() {
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
                receive_sequence_number: self.current_receive_sequence(),
                receive_con: ReceiveConnState::ConnectionValid,
                send_sequence_number: self.next_send_sequence(), // Use the same sequence number
                send_con: SendConnState::ConnectionValidAckRequest, // Request ACK again
            };
            return Ok(Some(SdoResponseData {
                client_info: self.client_info,
                seq_header: response_header,
                command,
            }));
        }

        if let Some((tid, code)) = abort_params {
            let abort_command = self.abort(tid, code); // This sets state to Closed
            let response_header = SequenceLayerHeader {
                receive_sequence_number: self.current_receive_sequence(),
                receive_con: ReceiveConnState::ConnectionValid,
                send_sequence_number: self.next_send_sequence(),
                send_con: SendConnState::NoConnection, // Closing connection
            };
            return Ok(Some(SdoResponseData {
                client_info: self.client_info,
                seq_header: response_header,
                command: abort_command,
            }));
        }

        Ok(None) // No action from tick
    }

    /// Processes an incoming SDO request payload (starting *directly* with the Sequence Layer header).
    ///
    /// Returns an `SdoResponseData` struct, which the caller is
    /// responsible for packaging into a transport-specific response.
    pub fn handle_request(
        &mut self,
        request_sdo_payload: &[u8], // Starts with Sequence Layer Header
        od: &mut ObjectDictionary,
        current_time_us: u64,
        command_handler: &mut dyn SdoCommandHandler,
    ) -> Result<SdoResponseData, PowerlinkError> {
        if request_sdo_payload.len() < 4 {
            return Err(PowerlinkError::BufferTooShort); // Need at least sequence header
        }
        trace!("Handling SDO request payload: {:?}", request_sdo_payload);

        let sequence_header = SequenceLayerHeader::deserialize(&request_sdo_payload[0..4])?;
        let command_payload = &request_sdo_payload[4..]; // Rest is command layer + data

        // --- Handle client retransmission request before sequence number processing ---
        if sequence_header.receive_con == ReceiveConnState::ErrorResponse {
            if let SdoServerState::SegmentedUpload(state) = self.state() {
                if let Some(last_command) = &state.last_sent_segment {
                    warn!(
                        "[SDO] Server: Client requested retransmission for TID {}. Resending last segment.",
                        state.transaction_id
                    );
                    let retransmit_header = SequenceLayerHeader {
                        receive_sequence_number: sequence_header.send_sequence_number, // Ack the request
                        receive_con: ReceiveConnState::ConnectionValid,
                        send_sequence_number: self.next_send_sequence(), // Resend with same seq number
                        send_con: SendConnState::ConnectionValidAckRequest,
                    };
                    return Ok(SdoResponseData {
                        client_info: self.client_info,
                        seq_header: retransmit_header,
                        command: last_command.clone(),
                    });
                }
            }
            // Ignore ErrorResponse if not in segmented upload or no segment stored
            warn!("[SDO] Server: Received ErrorResponse in unexpected state or missing segment.");
            // Send a basic ACK without processing command
            let ack_header = self.process_sequence_layer(sequence_header)?;
            let nil_command = SdoCommand {
                header: Default::default(),
                data_size: None,
                payload: Vec::new(),
            };
            return Ok(SdoResponseData {
                client_info: self.client_info,
                seq_header: ack_header,
                command: nil_command,
            });
        }

        debug!("Parsed SDO sequence header: {:?}", sequence_header);
        let mut response_header = self.process_sequence_layer(sequence_header)?;

        // Handle ACK-only or NIL command frames (no command payload)
        if command_payload.is_empty()
            && (*self.state() == SdoServerState::Opening
                || sequence_header.send_con == SendConnState::ConnectionValid)
        {
            debug!("Received ACK or NIL command.");
            if let SdoServerState::SegmentedUpload(mut state) = mem::take(self.state_mut()) {
                // Client ACK received, continue segmented upload.
                debug!("Client ACK received, continuing segmented upload.");
                let (response_command, is_last) =
                    state.get_next_upload_segment(od, current_time_us);

                // If not last, put state back. If last, transition to Closed.
                if !is_last {
                    *self.state_mut() = SdoServerState::SegmentedUpload(state);
                } else {
                    *self.state_mut() = SdoServerState::Closed; // Transfer complete
                }

                response_header.receive_sequence_number = self.current_receive_sequence();
                return Ok(SdoResponseData {
                    client_info: self.client_info,
                    seq_header: response_header,
                    command: response_command,
                });
            }
            // Just send back an ACK if Established and command payload is empty
            if self.state() == &SdoServerState::Established {
                let response_command = SdoCommand {
                    header: CommandLayerHeader {
                        transaction_id: 0, // Transaction ID might not be known here
                        is_response: true,
                        ..Default::default()
                    },
                    data_size: None,
                    payload: Vec::new(),
                };
                response_header.receive_sequence_number = self.current_receive_sequence();
                // This is a simple ACK, not part of a transfer, close connection
                self.state = SdoServerState::Closed;
                return Ok(SdoResponseData {
                    client_info: self.client_info,
                    seq_header: response_header,
                    command: response_command,
                });
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
                let response_command =
                    self.process_command_layer(sdo_command, od, current_time_us, command_handler);
                debug!("Generated SDO response command: {:?}", response_command);

                response_header.receive_sequence_number = self.current_receive_sequence();

                // If this is the end of a transfer, set state to Closed for pruning.
                if response_command.header.segmentation == Segmentation::Expedited
                    || response_command.header.segmentation == Segmentation::Complete
                    || response_command.header.is_aborted
                {
                    self.state = SdoServerState::Closed;
                }
                if response_command.header.is_aborted {
                    response_header.send_con = SendConnState::NoConnection;
                }

                // Return the response data components
                Ok(SdoResponseData {
                    client_info: self.client_info,
                    seq_header: response_header,
                    command: response_command,
                })
            }
            Err(e) => {
                // Failed to deserialize command - create an Abort response
                error!("Failed to deserialize SDO command payload: {:?}", e);
                // Try to get transaction ID from the raw payload if possible (best effort)
                let tid = command_payload.first().map_or(0, |flags| flags & 0x0F);
                let abort_command = self.abort(tid, 0x0504_0001); // Invalid command specifier
                response_header.receive_sequence_number = self.current_receive_sequence();
                // Abort implies connection closure at sequence layer for response
                response_header.send_con = SendConnState::NoConnection;
                // Abort sets state to Closed
                Ok(SdoResponseData {
                    client_info: self.client_info,
                    seq_header: response_header,
                    command: abort_command,
                })
            }
        }
    }

    /// Processes the SDO command, interacts with the OD, and returns a response command.
    /// This logic was moved from SdoServer.
    pub(super) fn process_command_layer(
        &mut self,
        command: SdoCommand,
        od: &mut ObjectDictionary,
        current_time_us: u64,
        command_handler: &mut dyn SdoCommandHandler,
    ) -> SdoCommand {
        // Temporarily take ownership of the state to avoid borrow checker issues.
        let current_state = mem::take(self.state_mut());

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
                        *self.state_mut() = SdoServerState::SegmentedUpload(state);
                    } else {
                        *self.state_mut() = SdoServerState::Established; // Will be set to Closed by caller
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
                    *self.state_mut() = SdoServerState::SegmentedUpload(state); // Put state back before aborting
                    return self.abort(command.header.transaction_id, 0x0504_0003); // Invalid sequence
                }
            } else if command.header.is_aborted {
                info!(
                    "Client aborted segmented upload (TID {}).",
                    command.header.transaction_id
                );
                // Client aborted, just transition back to Established (will be set to Closed by caller)
                *self.state_mut() = SdoServerState::Established;
                // Don't send another abort back
                return SdoCommand {
                    header: CommandLayerHeader {
                        is_aborted: true, // Set abort flag so caller closes connection
                        ..Default::default()
                    },
                    data_size: None,
                    payload: Vec::new(),
                }; // Empty response
            } else {
                error!(
                    "Mismatched transaction ID during segmented upload. Expected {}, got {}",
                    state.transaction_id, command.header.transaction_id
                );
                *self.state_mut() = SdoServerState::SegmentedUpload(state); // Put state back
                return self.abort(command.header.transaction_id, 0x0800_0000); // General error
            }
        } else {
            // Not in segmented upload, put state back and process command normally
            *self.state_mut() = current_state;
        }

        // Handle client abort received in Established or other states
        if command.header.is_aborted {
            info!(
                "Client aborted SDO transfer (TID {}).",
                command.header.transaction_id
            );
            *self.state_mut() = SdoServerState::Established; // Ensure state is reset (will be set to Closed by caller)
            // Don't send another abort back
            return SdoCommand {
                header: CommandLayerHeader {
                    is_aborted: true, // Set abort flag so caller closes connection
                    ..Default::default()
                },
                data_size: None,
                payload: Vec::new(),
            }; // Empty response
        }

        let response_header = CommandLayerHeader {
            transaction_id: command.header.transaction_id,
            is_response: true,
            ..Default::default()
        };

        match command.header.command_id {
            CommandId::ReadByIndex => {
                handlers::handle_read_by_index(self, command, response_header, od, current_time_us)
            }
            CommandId::WriteByIndex => {
                handlers::handle_write_by_index(self, command, response_header, od, current_time_us)
            }
            CommandId::ReadByName => {
                handlers::handle_read_by_name(self, command, response_header, od, current_time_us)
            }
            CommandId::WriteByName => {
                handlers::handle_write_by_name(self, command, response_header, od, current_time_us)
            }
            CommandId::ReadAllByIndex => handlers::handle_read_all_by_index(
                self,
                command,
                response_header,
                od,
                current_time_us,
            ),
            CommandId::ReadMultipleParamByIndex => handlers::handle_read_multiple_params(
                self,
                command,
                response_header,
                od,
                current_time_us,
            ),
            CommandId::MaxSegmentSize => {
                handlers::handle_max_segment_size(command, response_header)
            }
            // Delegate complex commands to the custom handler
            CommandId::WriteAllByIndex => command_handler.handle_write_all_by_index(command, od),
            CommandId::WriteMultipleParamByIndex => {
                command_handler.handle_write_multiple_params(command, od)
            }
            CommandId::FileRead => command_handler.handle_file_read(command, od),
            CommandId::FileWrite => command_handler.handle_file_write(command, od),
            CommandId::Nil => {
                debug!("Received NIL command, sending empty ACK.");
                SdoCommand {
                    header: response_header,
                    data_size: None,
                    payload: Vec::new(),
                }
            },
            CommandId::Abort => {
                error!("Received Abort command from client, but not handled here.");
                self.abort(command.header.transaction_id, 0x0504_0001) // Invalid command specifier
            }
        }
    }

    /// Updates the server state based on the incoming sequence layer header
    /// and determines the appropriate response header.
    ///
    /// This logic was moved from `SdoServer`.
    pub fn process_sequence_layer(
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
                    response.receive_con = ReceiveConnState::NoConnection;
                    response.send_con = SendConnState::NoConnection;
                    response.receive_sequence_number = request.send_sequence_number; // Echo back client seq
                    response.send_sequence_number = self.send_sequence_number; // Our (irrelevant) seq
                    // We return Ok, but the command layer processing will likely fail or do nothing
                }
            }
            SdoServerState::Opening => {
                // Client confirms connection with ConnectionValid and ACKs our Initialization seq num (0)
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
                        "Invalid sequence state during SDO opening. Client State: {:?}, Client Seq: {}, Client ACK: {}, Expected ACK: {}",
                        request.send_con,
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
            SdoServerState::Established
            | SdoServerState::SegmentedDownload(_)
            | SdoServerState::SegmentedUpload(_) => {
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
                // Let the command layer handle the empty payload.
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
                    // Acknowledgment received for an upload segment, so clear the timeout.
                    if let SdoServerState::SegmentedUpload(ref mut state) = self.state {
                        // Check if client ack matches the segment we sent
                        if request.receive_sequence_number == self.send_sequence_number {
                            state.deadline_us = None;
                            state.last_sent_segment = None; // Clear stored segment after ACK
                            state.retransmissions_left = 0; // Reset retry count
                        } else {
                            // Client ACKed something else, maybe an old frame? Ignore this ACK regarding timeout.
                            warn!(
                                "Received ACK ({}) for wrong segment during upload (expected {}).",
                                request.receive_sequence_number, self.send_sequence_number
                            );
                        }
                    }

                    // Increment send sequence number *only* if this wasn't a duplicate frame
                    // (The duplicate case above keeps the old send_sequence_number)
                    self.send_sequence_number = self.send_sequence_number.wrapping_add(1) % 64;

                    // Default response for valid sequence
                    response.receive_con = ReceiveConnState::ConnectionValid;
                    response.receive_sequence_number = self.last_received_sequence_number;
                    response.send_con = SendConnState::ConnectionValid; // Default, might be overridden by command layer
                    // If we are about to start a segmented upload, request ACK
                    if let SdoServerState::SegmentedUpload(ref state) = self.state {
                        if state.offset == 0 {
                            // First segment is about to be sent
                            response.send_con = SendConnState::ConnectionValidAckRequest;
                        }
                    }
                    response.send_sequence_number = self.send_sequence_number;
                }
            }
        }
        Ok(response) // Return the calculated response header
    }
}
