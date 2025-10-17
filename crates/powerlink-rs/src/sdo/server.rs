use crate::frame::ServiceId;
use crate::od::{ObjectDictionary, ObjectValue};
use crate::sdo::command::{
    CommandId, CommandLayerHeader, ReadByIndexRequest, SdoCommand, Segmentation,
    WriteByIndexRequest,
};
use crate::sdo::sequence::{ReceiveConnState, SendConnState, SequenceLayerHeader};
use crate::types::MessageType;
use crate::{Codec, PowerlinkError};
use alloc::vec::Vec;
use alloc::vec;


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
        // The first 4 bytes of the ASnd payload are the SDO message/service headers.
        if request_payload.len() < 8 {
            return Err(PowerlinkError::BufferTooShort);
        }
        let sequence_header = SequenceLayerHeader::deserialize(&request_payload[4..8])?;
        let command_payload = &request_payload[8..];

        let mut response_header = self.process_sequence_layer(sequence_header)?;
        let sdo_command = SdoCommand::deserialize(command_payload)?;

        // Process the command and generate a response command.
        let response_command = self.process_command_layer(sdo_command, od);

        // Acknowledge the received sequence number.
        response_header.receive_sequence_number = self.last_received_sequence_number;

        let mut response_buffer = vec![0u8; 1500]; // Allocate max size.
        // Re-create the ASnd SDO prefix.
        response_buffer[0] = MessageType::ASnd as u8;
        response_buffer[3] = ServiceId::Sdo as u8;
        response_header.serialize(&mut response_buffer[4..8])?;

        let written_len = response_command.serialize(&mut response_buffer[8..])?;
        response_buffer.truncate(8 + written_len);

        Ok(response_buffer)
    }

    /// Processes the SDO command, interacts with the OD, and returns a response command.
    fn process_command_layer(
        &mut self,
        command: SdoCommand,
        od: &mut ObjectDictionary,
    ) -> SdoCommand {
        let response_header = CommandLayerHeader {
            transaction_id: command.header.transaction_id,
            is_response: true,
            is_aborted: false,
            segmentation: Segmentation::Expedited, // Assume expedited for now
            command_id: command.header.command_id,
            segment_size: 0,
        };

        match command.header.command_id {
            CommandId::ReadByIndex => {
                match ReadByIndexRequest::from_payload(&command.payload) {
                    Ok(req) => match od.read(req.index, req.sub_index) {
                        Some(value) => {
                            let payload = value.serialize();
                            SdoCommand {
                                header: response_header,
                                data_size: None,
                                payload,
                            }
                        }
                        None => self.abort(
                            command.header.transaction_id,
                            0x0602_0000, // Object does not exist
                        ),
                    },
                    Err(_) => self.abort(command.header.transaction_id, 0x0800_0000), // General error
                }
            }
            CommandId::WriteByIndex => {
                match WriteByIndexRequest::from_payload(&command.payload) {
                    Ok(req) => {
                        // To write, we must first read the object to know its type,
                        // then deserialize the request data into that type.
                        match od.read(req.index, req.sub_index) {
                            Some(type_template) => {
                                match ObjectValue::deserialize(req.data, &type_template) {
                                    Ok(value) => {
                                        match od.write(req.index, req.sub_index, value) {
                                            Ok(_) => SdoCommand {
                                                header: response_header,
                                                data_size: None,
                                                payload: Vec::new(),
                                            },
                                            Err(_) => self.abort(
                                                command.header.transaction_id,
                                                0x0601_0002, // Attempt to write a read-only object
                                            ),
                                        }
                                    }
                                    Err(_) => {
                                        self.abort(command.header.transaction_id, 0x0607_0010)
                                    } // Data type mismatch
                                }
                            }
                            None => self.abort(
                                command.header.transaction_id,
                                0x0602_0000, // Object does not exist
                            ),
                        }
                    }
                    Err(_) => self.abort(command.header.transaction_id, 0x0800_0000), // General error
                }
            }
        }
    }

    /// Creates an SDO Abort command.
    fn abort(&self, transaction_id: u8, abort_code: u32) -> SdoCommand {
        SdoCommand {
            header: CommandLayerHeader {
                transaction_id,
                is_response: true,
                is_aborted: true,
                segmentation: Segmentation::Expedited,
                command_id: CommandId::ReadByIndex, // Command ID is arbitrary in aborts
                segment_size: 4,
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