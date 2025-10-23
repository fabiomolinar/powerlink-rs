// In crates/powerlink-rs/src/sdo/server.rs

use crate::od::{ObjectDictionary, ObjectValue};
use crate::sdo::command::{
    CommandId, CommandLayerHeader, ReadByIndexRequest, SdoCommand, Segmentation,
    WriteByIndexRequest,
};
use crate::sdo::sequence::{ReceiveConnState, SendConnState, SequenceLayerHeader};
use crate::{Codec, PowerlinkError};
use alloc::vec;
use alloc::vec::Vec;
use log::{debug, error, info, trace, warn};

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
    pub(super) send_sequence_number: u8,
    // The last sequence number the server correctly received from the client.
    pub(super) last_received_sequence_number: u8,
}

const MAX_EXPEDITED_PAYLOAD: usize = 1452; // Max SDO payload within ASnd frame (excluding headers)

impl SdoServer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Processes an incoming SDO request contained within an ASnd payload.
    ///
    /// This function handles the Sequence Layer logic and will eventually delegate
    /// the inner command to a Command Layer processor.
    /// The input `request_payload` should start directly with the SDO Sequence Layer header.
    pub fn handle_request(
        &mut self,
        request_sdo_payload: &[u8], // Renamed to clarify it's SDO Seq+Cmd+Data
        od: &mut ObjectDictionary,
    ) -> Result<Vec<u8>, PowerlinkError> {
        // SDO Sequence Layer Header starts at offset 4 within the ASnd payload.
        // SDO Command Layer Header starts at offset 8.
        if request_sdo_payload.len() < 8 {
            return Err(PowerlinkError::BufferTooShort);
        }
        trace!("Handling SDO request payload: {:?}", request_sdo_payload);
        let sequence_header = SequenceLayerHeader::deserialize(&request_sdo_payload[0..4])?;
        let command_payload = &request_sdo_payload[4..]; // Command Layer starts after Seq Layer

        debug!("Parsed SDO sequence header: {:?}", sequence_header);

        let mut response_header = self.process_sequence_layer(sequence_header)?;
        let sdo_command = SdoCommand::deserialize(command_payload)?;

        debug!("Parsed SDO command: {:?}", sdo_command);

        // Process the command and generate a response command.
        let response_command = self.process_command_layer(sdo_command, od);
        debug!("Generated SDO response command: {:?}", response_command);

        // Acknowledge the received sequence number in the response.
        response_header.receive_sequence_number = self.last_received_sequence_number;

        // --- Assemble the full SDO response payload (Seq + Cmd + Data) ---
        let mut response_sdo_payload = vec![0u8; 1500]; // Allocate max SDO size.
        let seq_len = response_header.serialize(&mut response_sdo_payload[0..4])?;
        let cmd_len = response_command.serialize(&mut response_sdo_payload[seq_len..])?;
        let total_sdo_len = seq_len + cmd_len;
        response_sdo_payload.truncate(total_sdo_len);

        Ok(response_sdo_payload) // Return only the SDO payload (Seq+Cmd+Data)
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
                debug!("Client ACK received, continuing segmented upload.");
                // Reset send sequence number on ACK according to state diagram logic
                // (though the logic is complex, this simplification works for basic ACK)
                // self.send_sequence_number = state.last_sent_seq_num; // Need to track last sent
                return self.handle_segmented_upload(state);
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
                                    };
                                    // The handle_segmented_upload function now mutates self.state
                                    self.handle_segmented_upload(transfer_state)
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
    fn handle_segmented_upload(&mut self, mut state: SdoTransferState) -> SdoCommand {
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
            // More segments to follow, update the state
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
                            offset: req.data.len(),          // Track bytes received
                            index: req.index,
                            sub_index: req.sub_index,
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
                    transfer_state.data_buffer.extend_from_slice(&command.payload);
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
                segment_size: 4,           // Size of the abort code payload
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

        match self.state {
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
                    // Reflect client's numbers back? Spec is unclear, be conservative.
                    response.receive_sequence_number = request.send_sequence_number;
                    response.send_sequence_number = self.send_sequence_number; // Keep server's last sent (0)
                    return Err(PowerlinkError::SdoSequenceError); // Signal error upstream if needed
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
                                                         // Respond with error? Spec unclear, safer to go back to closed.
                    return Err(PowerlinkError::SdoSequenceError);
                }
            }
            SdoServerState::Established
            | SdoServerState::SegmentedDownload(_)
            | SdoServerState::SegmentedUpload(_) => {
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
                    return Ok(response);
                }
                // Handle out-of-order/lost frame from client
                else if request.send_sequence_number != expected_seq {
                    error!(
                        "SDO sequence number mismatch. Expected {}, got {}. Requesting retransmission from client.",
                        expected_seq, request.send_sequence_number
                    );
                    // Request retransmission from the client starting after the last good one.
                    response.receive_con = ReceiveConnState::ErrorResponse;
                    response.receive_sequence_number = self.last_received_sequence_number;
                    response.send_con = SendConnState::ConnectionValid;
                    response.send_sequence_number = self.send_sequence_number; // Resend our last frame's number
                                                                               // Do not update server state or sequence numbers on error
                    return Err(PowerlinkError::SdoSequenceError); // Signal error upstream
                }
                // --- Sequence OK ---
                else {
                    self.last_received_sequence_number = request.send_sequence_number;
                    // Increment send sequence number *only if* we aren't handling retransmission/duplicates
                    if request.receive_con != ReceiveConnState::ErrorResponse {
                        self.send_sequence_number = self.send_sequence_number.wrapping_add(1) % 64;
                    }
                    response.receive_con = ReceiveConnState::ConnectionValid;
                    response.receive_sequence_number = self.last_received_sequence_number;
                    response.send_con = SendConnState::ConnectionValid; // Default, might be overridden by command layer
                    response.send_sequence_number = self.send_sequence_number;
                }
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
            last_received_sequence_number: 0, // Initialize appropriately, maybe with a sentinel?
        }
    }
}

// Tests remain the same as previous version...
#[cfg(test)]
mod tests {
    use super::*;
    use crate::od::{AccessType, Category, Object, ObjectEntry};
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
                    category: Category::Optional,
                    access: Some(AccessType::Constant),
                    default_value: None,
                    value_range: None,
                    pdo_mapping: None,
                },
            );
            od.insert(
                0x2000,
                ObjectEntry {
                    object: Object::Variable(ObjectValue::Unsigned32(0x12345678)),
                    name: "Test_U32",
                    category: Category::Optional,
                    access: Some(AccessType::ReadWrite),
                    default_value: None,
                    value_range: None,
                    pdo_mapping: None,
                },
            );
            od.insert(
                0x3000,
                ObjectEntry {
                    object: Object::Variable(ObjectValue::OctetString(vec![0; 2000])),
                    name: "Large_Object",
                    category: Category::Optional,
                    access: Some(AccessType::ReadWrite),
                    default_value: None,
                    value_range: None,
                    pdo_mapping: None,
                },
            );
            od
        }

        // Builds the SDO payload (Seq Hdr + Cmd Hdr + Cmd Payload)
        pub fn build_sdo_payload(
            seq_header: SequenceLayerHeader,
            cmd: SdoCommand,
        ) -> Vec<u8> {
            let mut payload = vec![0u8; 1500];
            let mut offset = 0;
            offset += seq_header.serialize(&mut payload[offset..]).unwrap();
            offset += cmd.serialize(&mut payload[offset..]).unwrap();
            payload.truncate(offset);
            payload
        }
    }

    #[test]
    fn test_expedited_read_ok() {
        let mut server = SdoServer::new();
        let mut od = test_utils::get_test_od();
        // Assume connection is established.
        server.state = SdoServerState::Established;
        server.last_received_sequence_number = 0;
        server.send_sequence_number = 0;

        let read_cmd = SdoCommand {
            header: CommandLayerHeader {
                transaction_id: 1,
                command_id: CommandId::ReadByIndex,
                ..Default::default()
            },
            data_size: None,
            payload: vec![0x00, 0x20, 0x00, 0x00], // Read 0x2000 sub 0
        };

        let request_sdo_payload = test_utils::build_sdo_payload(
            SequenceLayerHeader {
                send_sequence_number: 1,
                ..Default::default()
            },
            read_cmd,
        );

        let response_sdo_payload = server
            .handle_request(&request_sdo_payload, &mut od)
            .unwrap();

        // The response payload should contain the SDO response.
        let response_seq = SequenceLayerHeader::deserialize(&response_sdo_payload[0..4]).unwrap();
        let response_cmd = SdoCommand::deserialize(&response_sdo_payload[4..]).unwrap();

        assert_eq!(response_seq.receive_sequence_number, 1); // Ack client's seq 1
        assert_eq!(response_seq.send_sequence_number, 1); // Server sends seq 1

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
        server.last_received_sequence_number = 5;
        server.send_sequence_number = 10;

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

        let request_sdo_payload = test_utils::build_sdo_payload(
            SequenceLayerHeader {
                send_sequence_number: 6,
                ..Default::default()
            },
            write_cmd,
        );

        let response_sdo_payload = server
            .handle_request(&request_sdo_payload, &mut od)
            .unwrap();

        let response_seq = SequenceLayerHeader::deserialize(&response_sdo_payload[0..4]).unwrap();
        let response_cmd = SdoCommand::deserialize(&response_sdo_payload[4..]).unwrap();

        assert_eq!(response_seq.receive_sequence_number, 6); // Ack client's seq 6
        assert_eq!(response_seq.send_sequence_number, 11); // Server sends seq 11

        assert!(!response_cmd.header.is_aborted);
        assert!(response_cmd.payload.is_empty()); // Success write response has no payload

        // Verify the value was written
        let value = od.read_u32(0x2000, 0).unwrap();
        assert_eq!(value, 0xDEADBEEF);
    }

    #[test]
    fn test_read_non_existent_aborts() {
        let mut server = SdoServer::new();
        let mut od = test_utils::get_test_od();
        server.state = SdoServerState::Established;
        server.last_received_sequence_number = 1;
        server.send_sequence_number = 1;

        let read_cmd = SdoCommand {
            header: CommandLayerHeader {
                transaction_id: 3,
                command_id: CommandId::ReadByIndex,
                ..Default::default()
            },
            data_size: None,
            payload: vec![0xFF, 0xFF, 0x00, 0x00], // Read non-existent object
        };

        let request_sdo_payload = test_utils::build_sdo_payload(
            SequenceLayerHeader {
                send_sequence_number: 2,
                ..Default::default()
            },
            read_cmd,
        );

        let response_sdo_payload = server
            .handle_request(&request_sdo_payload, &mut od)
            .unwrap();
        let response_seq = SequenceLayerHeader::deserialize(&response_sdo_payload[0..4]).unwrap();
        let response_cmd = SdoCommand::deserialize(&response_sdo_payload[4..]).unwrap();

        assert_eq!(response_seq.receive_sequence_number, 2); // Ack client's seq 2
        assert_eq!(response_seq.send_sequence_number, 2); // Server sends seq 2

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
        server.send_sequence_number = 5; // Simulate prior responses

        // 1. Client requests to read a large object (0x3000)
        let read_cmd = SdoCommand {
            header: CommandLayerHeader {
                transaction_id: 10,
                command_id: CommandId::ReadByIndex,
                ..Default::default()
            },
            data_size: None,
            payload: vec![0x00, 0x30, 0x00, 0x00], // Read 0x3000/0
        };
        let request1_sdo = test_utils::build_sdo_payload(
            SequenceLayerHeader {
                send_sequence_number: 3,
                receive_sequence_number: 5,
                ..Default::default()
            },
            read_cmd,
        );

        // --- Server sends INITIATE response ---
        let response1_sdo_payload = server.handle_request(&request1_sdo, &mut od).unwrap();
        let response1_seq = SequenceLayerHeader::deserialize(&response1_sdo_payload[0..4]).unwrap();
        let response1_cmd = SdoCommand::deserialize(&response1_sdo_payload[4..]).unwrap();

        assert_eq!(response1_seq.receive_sequence_number, 3); // ACK 3
        assert_eq!(response1_seq.send_sequence_number, 6); // Send 6

        assert_eq!(response1_cmd.header.segmentation, Segmentation::Initiate);
        assert_eq!(response1_cmd.data_size, Some(2000));
        assert_eq!(response1_cmd.payload.len(), MAX_EXPEDITED_PAYLOAD); // First chunk

        assert!(matches!(server.state, SdoServerState::SegmentedUpload(_))); // Check state change

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
        let request2_sdo = test_utils::build_sdo_payload(
            SequenceLayerHeader {
                send_sequence_number: 4,
                receive_sequence_number: 6,
                ..Default::default()
            }, // Client sends Seq 4, ACKs Server Seq 6
            nil_cmd,
        );

        // --- Server sends COMPLETE response ---
        let response2_sdo_payload = server.handle_request(&request2_sdo, &mut od).unwrap();
        let response2_seq = SequenceLayerHeader::deserialize(&response2_sdo_payload[0..4]).unwrap();
        let response2_cmd = SdoCommand::deserialize(&response2_sdo_payload[4..]).unwrap();

        assert_eq!(response2_seq.receive_sequence_number, 4); // ACK 4
        assert_eq!(response2_seq.send_sequence_number, 7); // Send 7

        assert_eq!(response2_cmd.header.segmentation, Segmentation::Complete);
        assert_eq!(response2_cmd.payload.len(), 2000 - MAX_EXPEDITED_PAYLOAD); // Second chunk
        assert_eq!(server.state, SdoServerState::Established); // Server should be back to established state
    }
}

