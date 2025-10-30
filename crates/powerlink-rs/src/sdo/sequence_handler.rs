// crates/powerlink-rs/src/sdo/sequence_handler.rs
use crate::sdo::sequence::{ReceiveConnState, SendConnState, SequenceLayerHeader};
use crate::sdo::state::SdoServerState;
use crate::PowerlinkError;
use log::{debug, error, info, warn};

/// Manages the SDO Sequence Layer logic, including connection state and sequence numbers.
#[derive(Debug, Default)]
pub struct SdoSequenceHandler {
    state: SdoServerState,
    send_sequence_number: u8,
    last_received_sequence_number: u8,
}

impl SdoSequenceHandler {
    pub fn new() -> Self {
        Self {
            state: SdoServerState::Closed,
            send_sequence_number: 0,
            last_received_sequence_number: 63, // Set to 63 (equiv to -1) so first received seq (0) is valid
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

    /// Resets the connection state, typically after an abort.
    pub fn reset(&mut self) {
        self.state = SdoServerState::Established;
        // Keep sequence numbers as they are, but reset state.
        // A new Initialization would reset them fully.
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
