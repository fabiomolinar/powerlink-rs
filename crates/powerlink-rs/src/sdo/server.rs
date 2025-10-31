use crate::frame::basic::MacAddress;
use crate::sdo::OD_IDX_SDO_TIMEOUT;
use crate::sdo::command::{
    CommandId, CommandLayerHeader, DefaultSdoHandler, SdoCommand, SdoCommandHandler, Segmentation,
};
use crate::sdo::handlers;
use crate::sdo::sequence::{ReceiveConnState, SendConnState, SequenceLayerHeader};
use crate::sdo::sequence_handler::SdoSequenceHandler;
use crate::sdo::state::SdoServerState;
use crate::sdo::transport::SdoResponseData;
#[cfg(feature = "sdo-udp")]
use crate::types::IpAddress;
use crate::{PowerlinkError, od::ObjectDictionary};
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
    pub(crate) sequence_handler: SdoSequenceHandler,
    /// Optional handler for vendor-specific or complex commands.
    handler: Box<dyn SdoCommandHandler>,
    /// Information about the current or last connected client. Needed for server-initiated aborts.
    current_client_info: Option<SdoClientInfo>,
}

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
    ) -> Result<Option<SdoResponseData>, PowerlinkError> {
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
                return Ok(Some(SdoResponseData {
                    client_info,
                    seq_header: response_header,
                    command,
                }));
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
                return Ok(Some(SdoResponseData {
                    client_info,
                    seq_header: response_header,
                    command: abort_command,
                }));
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
    /// Returns an `SdoResponseData` struct, which the caller is
    /// responsible for packaging into a transport-specific response.
    pub fn handle_request(
        &mut self,
        request_sdo_payload: &[u8], // Starts with Sequence Layer Header
        client_info: SdoClientInfo, // Pass transport info
        od: &mut ObjectDictionary,
        current_time_us: u64,
    ) -> Result<SdoResponseData, PowerlinkError> {
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
                    return Ok(SdoResponseData {
                        client_info,
                        seq_header: retransmit_header,
                        command: last_command.clone(),
                    });
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
            return Ok(SdoResponseData {
                client_info,
                seq_header: ack_header,
                command: nil_command,
            });
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
                return Ok(SdoResponseData {
                    client_info,
                    seq_header: response_header,
                    command: response_command,
                });
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
                return Ok(SdoResponseData {
                    client_info,
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
                Ok(SdoResponseData {
                    client_info,
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
                response_header.receive_sequence_number =
                    self.sequence_handler.current_receive_sequence();
                // Abort implies connection closure at sequence layer for response
                response_header.send_con = SendConnState::NoConnection;
                // Abort clears client info
                self.current_client_info = None;
                Ok(SdoResponseData {
                    client_info,
                    seq_header: response_header,
                    command: abort_command,
                })
            }
        }
    }

    /// Processes the SDO command, interacts with the OD, and returns a response command.
    pub(super) fn process_command_layer(
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
                        *self.sequence_handler.state_mut() = SdoServerState::SegmentedUpload(state);
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
                handlers::handle_max_segment_size(self, command, response_header)
            }
            // Delegate complex commands to the custom handler
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

    /// Creates an SDO Abort command. Resets internal state.
    pub(super) fn abort(&mut self, transaction_id: u8, abort_code: u32) -> SdoCommand {
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
