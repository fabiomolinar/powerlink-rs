use crate::frame::ServiceId;
use crate::od::{ObjectDictionary, ObjectValue};
use crate::sdo::command::{
    CommandId, CommandLayerHeader, ReadByIndexRequest, SdoCommand, Segmentation,
    WriteByIndexRequest,
};
use crate::sdo::sequence::{ReceiveConnState, SendConnState, SequenceLayerHeader};
use crate::types::MessageType;
use crate::{Codec, PowerlinkError};
use alloc::vec;
use alloc::vec::Vec;

/// The state of an SDO connection from the server's perspective.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
enum SdoServerState {
    #[default]
    Closed,
    Opening,
    Established,
    SegmentedDownload(SdoTransferState),
    // SegmentedUpload is a future implementation
}

/// Holds the context for an ongoing segmented transfer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdoTransferState {
    transaction_id: u8,
    total_size: usize,
    data_buffer: Vec<u8>,
    index: u16,
    sub_index: u8,
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
                    Ok(req) => match od.read(req.index, req.sub_index) {
                        Some(value) => {
                            let payload = value.serialize();
                            // TODO: Add support for segmented uploads.
                            response_header.segment_size = payload.len() as u16;
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
                self.handle_write_by_index(command, response_header, od)
            }
        }
    }

    fn handle_write_by_index(
        &mut self,
        command: SdoCommand,
        response_header: CommandLayerHeader,
        od: &mut ObjectDictionary,
    ) -> SdoCommand {
        match command.header.segmentation {
            Segmentation::Expedited => {
                // Handle a complete write in a single frame.
                match WriteByIndexRequest::from_payload(&command.payload) {
                    Ok(req) => match self.write_to_od(req.index, req.sub_index, req.data, od) {
                        Ok(_) => SdoCommand {
                            header: response_header,
                            data_size: None,
                            payload: Vec::new(),
                        },
                        Err(abort_code) => self.abort(command.header.transaction_id, abort_code),
                    },
                    Err(_) => self.abort(command.header.transaction_id, 0x0800_0000), // General error
                }
            }
            Segmentation::Initiate => {
                // Start a new segmented download.
                match WriteByIndexRequest::from_payload(&command.payload) {
                    Ok(req) => {
                        self.state = SdoServerState::SegmentedDownload(SdoTransferState {
                            transaction_id: command.header.transaction_id,
                            total_size: command.data_size.unwrap_or(0) as usize,
                            data_buffer: req.data.to_vec(),
                            index: req.index,
                            sub_index: req.sub_index,
                        });
                        SdoCommand {
                            header: response_header,
                            data_size: None,
                            payload: Vec::new(),
                        }
                    }
                    Err(_) => self.abort(command.header.transaction_id, 0x0800_0000), // General error
                }
            }
            Segmentation::Segment | Segmentation::Complete => {
                if let SdoServerState::SegmentedDownload(ref mut transfer_state) = self.state {
                    if transfer_state.transaction_id != command.header.transaction_id {
                        return self.abort(command.header.transaction_id, 0x0800_0000); // Mismatched transaction
                    }

                    transfer_state.data_buffer.extend_from_slice(&command.payload);

                    if command.header.segmentation == Segmentation::Complete {
                        // All segments received, finalize the write.
                        let state = transfer_state.clone();
                        let result =
                            self.write_to_od(state.index, state.sub_index, &state.data_buffer, od);
                        self.state = SdoServerState::Established; // Return to established state
                        match result {
                            Ok(_) => SdoCommand {
                                header: response_header,
                                data_size: None,
                                payload: Vec::new(),
                            },
                            Err(abort_code) => self.abort(command.header.transaction_id, abort_code),
                        }
                    } else {
                        // Acknowledge the segment.
                        SdoCommand {
                            header: response_header,
                            data_size: None,
                            payload: Vec::new(),
                        }
                    }
                } else {
                    self.abort(command.header.transaction_id, 0x0800_0000) // Unexpected segment
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
        match od.read(index, sub_index) {
            Some(type_template) => match ObjectValue::deserialize(data, &type_template) {
                Ok(value) => match od.write(index, sub_index, value) {
                    Ok(_) => Ok(()),
                    Err(_) => Err(0x0601_0002), // Attempt to write a read-only object
                },
                Err(_) => Err(0x0607_0010), // Data type mismatch
            },
            None => Err(0x0602_0000), // Object does not exist
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
            SdoServerState::Established | SdoServerState::SegmentedDownload(_) => {
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