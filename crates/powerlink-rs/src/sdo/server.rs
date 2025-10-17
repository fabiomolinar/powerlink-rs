// In crates/powerlink-rs/src/sdo/server.rs

use crate::od::ObjectDictionary;
use crate::sdo::sequence::{SendConnState, ReceiveConnState, SequenceLayerHeader};
use crate::PowerlinkError;
use crate::frame::{codec::Codec, ServiceId};
use crate::types::{MessageType};

/// The state of an SDO connection from the server's perspective.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum SdoServerState {
    #[default]
    Closed,
    Opening,
    Established,
}

/// Manages a single SDO server connection.
///
/// A full implementation would use a BTreeMap or similar to manage multiple
/// connections, keyed by a client identifier (like NodeId or a socket address).
/// For now, this struct manages a single connection for simplicity.
pub struct SdoServer {
    state: SdoServerState,
    // The next sequence number this server will send.
    send_sequence_number: u8,
    // The last sequence number the server correctly received from the client.
    last_received_sequence_number: u8,
}

impl SdoServer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Processes an incoming SDO request contained within an ASnd payload.
    ///
    /// This function handles the Sequence Layer logic and will eventually delegate
    /// the inner command to a Command Layer processor.
    pub fn handle_request(
        &mut self,
        request_payload: &[u8],
        od: &mut ObjectDictionary,
    ) -> Result<Vec<u8>, PowerlinkError> {
        let sequence_header = SequenceLayerHeader::deserialize(&request_payload[4..8])?;
        let command_payload = &request_payload[8..];

        let response_header = self.process_sequence_layer(sequence_header)?;

        // TODO: Process the command_payload using the command layer logic.
        // For now, we just acknowledge the sequence layer.
        let response_command = Vec::new(); // Placeholder for command response.

        let mut response_buffer = vec![0u8; 8 + response_command.len()];
        // Re-create the ASnd SDO prefix.
        response_buffer[0] = MessageType::ASnd as u8;
        response_buffer[3] = ServiceId::Sdo as u8;
        response_header.serialize(&mut response_buffer[4..8])?;
        response_buffer[8..].copy_from_slice(&response_command);

        Ok(response_buffer)
    }

    /// Updates the server state based on the incoming sequence layer header
    /// and determines the appropriate response header.
    fn process_sequence_layer(
        &mut self,
        request: SequenceLayerHeader,
    ) -> Result<SequenceLayerHeader, PowerlinkError> {
        match self.state {
            SdoServerState::Closed => {
                if request.send_con == SendConnState::Initialization {
                    self.state = SdoServerState::Opening;
                    self.send_sequence_number = request.send_sequence_number;
                    self.last_received_sequence_number = request.send_sequence_number;
                    Ok(SequenceLayerHeader {
                        receive_con: ReceiveConnState::Initialization,
                        receive_sequence_number: self.last_received_sequence_number,
                        send_con: SendConnState::Initialization,
                        send_sequence_number: self.send_sequence_number,
                    })
                } else {
                    // Ignore requests on a closed connection that aren't for initialization.
                    Err(PowerlinkError::SdoSequenceError)
                }
            }
            SdoServerState::Opening => {
                if request.send_con == SendConnState::ConnectionValid {
                    self.state = SdoServerState::Established;
                    self.last_received_sequence_number = request.send_sequence_number;
                    Ok(SequenceLayerHeader {
                        receive_con: ReceiveConnState::ConnectionValid,
                        receive_sequence_number: self.last_received_sequence_number,
                        send_con: SendConnState::ConnectionValid,
                        send_sequence_number: self.send_sequence_number,
                    })
                } else {
                    Err(PowerlinkError::SdoSequenceError)
                }
            }
            SdoServerState::Established => {
                // A simplified check for lost frames. A full implementation would be more robust.
                let expected_seq = self.last_received_sequence_number.wrapping_add(1) % 64;
                if request.send_sequence_number != expected_seq {
                    // Request retransmission.
                    return Ok(SequenceLayerHeader {
                        receive_con: ReceiveConnState::ErrorResponse,
                        receive_sequence_number: self.last_received_sequence_number,
                        send_con: SendConnState::ConnectionValid,
                        send_sequence_number: self.send_sequence_number,
                    });
                }
                self.last_received_sequence_number = request.send_sequence_number;
                self.send_sequence_number = self.send_sequence_number.wrapping_add(1) % 64;

                Ok(SequenceLayerHeader {
                    receive_con: ReceiveConnState::ConnectionValid,
                    receive_sequence_number: self.last_received_sequence_number,
                    send_con: SendConnState::ConnectionValid,
                    send_sequence_number: self.send_sequence_number,
                })
            }
        }
    }
}

impl Default for SdoServer {
    fn default() -> Self {
        Self {
            state: SdoServerState::Closed,
            send_sequence_number: 0,
            last_received_sequence_number: 0,
        }
    }
}