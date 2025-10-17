// In crates/powerlink-rs/src/sdo/server.rs

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
    SegmentedUpload(SdoTransferState),
}

/// Holds the context for an ongoing segmented transfer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdoTransferState {
    transaction_id: u8,
    total_size: usize,
    data_buffer: Vec<u8>,
    // For uploads, this is the offset of the next byte to be sent.
    // For downloads, this tracks bytes received.
    offset: usize,
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

const MAX_EXPEDITED_PAYLOAD: usize = 1452;

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
        // Temporarily take ownership of the state to avoid borrow checker issues.
        let current_state = core::mem::take(&mut self.state);

        // If we are in a segmented upload, any new valid command from the client
        // just serves as an ACK to trigger the next segment.
        if let SdoServerState::SegmentedUpload(state) = current_state {
            if state.transaction_id == command.header.transaction_id {
                return self.handle_segmented_upload(state);
            }
            // If transaction ID doesn't match, restore state and fall through.
            self.state = SdoServerState::SegmentedUpload(state);
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
                    Ok(req) => match od.read(req.index, req.sub_index) {
                        Some(value) => {
                            let payload = value.serialize();
                            if payload.len() <= MAX_EXPEDITED_PAYLOAD {
                                // Expedited transfer
                                response_header.segment_size = payload.len() as u16;
                                SdoCommand {
                                    header: response_header,
                                    data_size: None,
                                    payload,
                                }
                            } else {
                                // Initiate segmented transfer
                                let mut transfer_state = SdoTransferState {
                                    transaction_id: command.header.transaction_id,
                                    total_size: payload.len(),
                                    data_buffer: payload,
                                    offset: 0,
                                    index: req.index,
                                    sub_index: req.sub_index,
                                };
                                let response = self.handle_segmented_upload(transfer_state.clone());
                                // The handle function will set offset, update state.
                                transfer_state.offset += response.payload.len();
                                self.state = SdoServerState::SegmentedUpload(transfer_state);
                                response
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
            CommandId::Nil => {
                // A NIL command acts as an ACK, often used during segmented uploads.
                // The main logic for handling this is already at the start of this function.
                // If we reach here, it means we're not in a segmented upload, so just send an empty ack.
                SdoCommand {
                    header: response_header,
                    data_size: None,
                    payload: Vec::new(),
                }
            }
        }
    }

    fn handle_segmented_upload(&mut self, mut state: SdoTransferState) -> SdoCommand {
        let mut response_header = CommandLayerHeader {
            transaction_id: state.transaction_id,
            is_response: true,
            is_aborted: false,
            segmentation: Segmentation::Segment, // Default
            command_id: CommandId::ReadByIndex,
            segment_size: 0,
        };

        let chunk_size = MAX_EXPEDITED_PAYLOAD;
        let remaining = state.total_size - state.offset;
        let current_chunk_size = chunk_size.min(remaining);
        // Clone the data into an owned Vec to release the borrow on `state`.
        let chunk = state.data_buffer[state.offset..state.offset + current_chunk_size].to_vec();

        let data_size = if state.offset == 0 {
            response_header.segmentation = Segmentation::Initiate;
            Some(state.total_size as u32)
        } else {
            None
        };

        state.offset += current_chunk_size;

        if state.offset >= state.total_size {
            response_header.segmentation = Segmentation::Complete;
            self.state = SdoServerState::Established;
        } else {
            // Move the updated state back into self.
            self.state = SdoServerState::SegmentedUpload(state);
        }

        response_header.segment_size = chunk.len() as u16;

        SdoCommand {
            header: response_header,
            data_size,
            payload: chunk,
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
                            offset: req.data.len(),
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
                    transfer_state.offset += command.payload.len();

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
            SdoServerState::Established
            | SdoServerState::SegmentedDownload(_)
            | SdoServerState::SegmentedUpload(_) => {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::od::{AccessType, Object, ObjectEntry};
    use alloc::string::ToString;

    // Helper functions and structs for testing.
    mod test_utils {
        use super::*;

        pub fn get_test_od() -> ObjectDictionary<'static> {
            let mut od = ObjectDictionary::new(None);
            od.insert(
                0x1008,
                ObjectEntry {
                    object: Object::Variable(ObjectValue::VisibleString("Device".to_string())),
                    name: "NMT_ManufactDevName_VS",
                    access: AccessType::Constant,
                },
            );
            od.insert(
                0x2000,
                ObjectEntry {
                    object: Object::Variable(ObjectValue::Unsigned32(0x12345678)),
                    name: "Test_U32",
                    access: AccessType::ReadWrite,
                },
            );
            od.insert(
                0x3000,
                ObjectEntry {
                    object: Object::Variable(ObjectValue::OctetString(vec![0; 2000])),
                    name: "Large_Object",
                    access: AccessType::ReadWrite,
                },
            );
            od
        }

        pub fn build_sdo_payload(
            seq_header: SequenceLayerHeader,
            cmd: SdoCommand,
        ) -> Vec<u8> {
            let mut payload = vec![0u8; 1500];
            payload[0] = MessageType::ASnd as u8;
            payload[3] = ServiceId::Sdo as u8;
            seq_header.serialize(&mut payload[4..8]).unwrap();
            let len = cmd.serialize(&mut payload[8..]).unwrap();
            payload.truncate(8 + len);
            payload
        }
    }

    #[test]
    fn test_expedited_read_ok() {
        let mut server = SdoServer::new();
        let mut od = test_utils::get_test_od();
        // Assume connection is established.
        server.state = SdoServerState::Established;

        let read_cmd = SdoCommand {
            header: CommandLayerHeader {
                transaction_id: 1,
                command_id: CommandId::ReadByIndex,
                ..Default::default()
            },
            data_size: None,
            payload: vec![0x00, 0x20, 0x00, 0x00], // Read 0x2000 sub 0
        };

        let request = test_utils::build_sdo_payload(Default::default(), read_cmd);
        let response = server.handle_request(&request, &mut od).unwrap();

        // The response payload should start at byte 12 (4 for ASnd header, 8 for SDO headers)
        let response_cmd = SdoCommand::deserialize(&response[8..]).unwrap();
        assert!(!response_cmd.header.is_aborted);
        assert_eq!(
            response_cmd.payload,
            0x12345678_u32.to_le_bytes().to_vec()
        );
    }

    #[test]
    fn test_expedited_write_ok() {
        let mut server = SdoServer::new();
        let mut od = test_utils::get_test_od();
        server.state = SdoServerState::Established;

        let write_cmd = SdoCommand {
            header: CommandLayerHeader {
                transaction_id: 2,
                command_id: CommandId::WriteByIndex,
                ..Default::default()
            },
            data_size: None,
            payload: {
                let mut p = vec![0x00, 0x20, 0x00, 0x00]; // Write 0x2000 sub 0
                p.extend_from_slice(&0xDEADBEEF_u32.to_le_bytes());
                p
            },
        };

        let request = test_utils::build_sdo_payload(Default::default(), write_cmd);
        server.handle_request(&request, &mut od).unwrap();

        // Verify the value was written
        let value = od.read_u32(0x2000, 0).unwrap();
        assert_eq!(value, 0xDEADBEEF);
    }

    #[test]
    fn test_read_non_existent_aborts() {
        let mut server = SdoServer::new();
        let mut od = test_utils::get_test_od();
        server.state = SdoServerState::Established;

        let read_cmd = SdoCommand {
            header: CommandLayerHeader {
                transaction_id: 3,
                command_id: CommandId::ReadByIndex,
                ..Default::default()
            },
            data_size: None,
            payload: vec![0xFF, 0xFF, 0x00, 0x00], // Read non-existent object
        };

        let request = test_utils::build_sdo_payload(Default::default(), read_cmd);
        let response = server.handle_request(&request, &mut od).unwrap();
        let response_cmd = SdoCommand::deserialize(&response[8..]).unwrap();

        assert!(response_cmd.header.is_aborted);
        let abort_code = u32::from_le_bytes(response_cmd.payload.try_into().unwrap());
        assert_eq!(abort_code, 0x0602_0000); // Object does not exist
    }

    #[test]
    fn test_segmented_upload() {
        let mut server = SdoServer::new();
        let mut od = test_utils::get_test_od();
        server.state = SdoServerState::Established;
        server.last_received_sequence_number = 2; // Simulate prior commands

        // 1. Client requests to read a large object
        let read_cmd = SdoCommand {
            header: CommandLayerHeader {
                transaction_id: 10,
                command_id: CommandId::ReadByIndex,
                ..Default::default()
            },
            data_size: None,
            payload: vec![0x00, 0x30, 0x00, 0x00], // Read 0x3000
        };
        let request1 = test_utils::build_sdo_payload(
            SequenceLayerHeader {
                send_sequence_number: 3,
                ..Default::default()
            },
            read_cmd,
        );

        // --- Server sends INITIATE response ---
        let response1_payload = server.handle_request(&request1, &mut od).unwrap();
        let response1_cmd = SdoCommand::deserialize(&response1_payload[8..]).unwrap();
        assert_eq!(response1_cmd.header.segmentation, Segmentation::Initiate);
        assert_eq!(response1_cmd.data_size, Some(2000));
        assert_eq!(response1_cmd.payload.len(), MAX_EXPEDITED_PAYLOAD);

        // 2. Client acknowledges by sending a NIL command
        let nil_cmd = SdoCommand {
            header: CommandLayerHeader {
                transaction_id: 10,
                command_id: CommandId::Nil,
                ..Default::default()
            },
            data_size: None,
            payload: Vec::new(),
        };
        let request2 = test_utils::build_sdo_payload(
            SequenceLayerHeader {
                send_sequence_number: 4,
                ..Default::default()
            },
            nil_cmd,
        );

        // --- Server sends COMPLETE response ---
        let response2_payload = server.handle_request(&request2, &mut od).unwrap();
        let response2_cmd = SdoCommand::deserialize(&response2_payload[8..]).unwrap();
        assert_eq!(response2_cmd.header.segmentation, Segmentation::Complete);
        assert_eq!(response2_cmd.payload.len(), 2000 - MAX_EXPEDITED_PAYLOAD);
        assert_eq!(server.state, SdoServerState::Established); // Server should be back to established state
    }
}