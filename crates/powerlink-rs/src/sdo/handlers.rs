use log::{error, info, warn};

use crate::PowerlinkError;
use crate::od::ObjectDictionary;
use crate::sdo::command::{
    CommandLayerHeader, ReadByIndexRequest, ReadByNameRequest,
    ReadMultipleParamRequest, SdoCommand, Segmentation, WriteByIndexRequest,
    WriteByNameRequest
};
use crate::sdo::state::{SdoServerState, SdoTransferState};

use super::SdoServer;
use alloc::vec::Vec;

use crate::sdo::OD_IDX_SDO_TIMEOUT;
const MAX_EXPEDITED_PAYLOAD: usize = 1452;

pub(super) fn handle_read_by_index(
    server: &mut SdoServer,
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
                            *server.sequence_handler.state_mut() =
                                SdoServerState::SegmentedUpload(transfer_state);
                        }
                        // Return the first segment
                        response_command
                    }
                }
                // Map OD read errors (Object/SubObjectNotFound) to SDO Abort codes
                None if od.read_object(req.index).is_none() => {
                    server.abort(command.header.transaction_id, 0x0602_0000) // Object does not exist
                }
                None => {
                    server.abort(command.header.transaction_id, 0x0609_0011) // Sub-index does not exist
                }
            }
        }
        Err(PowerlinkError::SdoInvalidCommandPayload) => {
            server.abort(command.header.transaction_id, 0x0504_0001) // Command specifier invalid
        }
        Err(_) => {
            // Other parsing errors
            server.abort(command.header.transaction_id, 0x0800_0000) // General error
        }
    }
}

pub(super) fn handle_write_by_index(
    server: &mut SdoServer,
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
                        Err(abort_code) => server.abort(command.header.transaction_id, abort_code),
                    }
                }
                Err(PowerlinkError::SdoInvalidCommandPayload) => {
                    server.abort(command.header.transaction_id, 0x0504_0001) // Command specifier invalid
                }
                Err(_) => server.abort(command.header.transaction_id, 0x0800_0000), // General error parsing payload
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
                        return server.abort(command.header.transaction_id, 0x0607_0010); // Type mismatch/length error
                    }
                    let timeout_ms = od.read_u32(OD_IDX_SDO_TIMEOUT, 0).unwrap_or(15000) as u64;
                    *server.sequence_handler.state_mut() =
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
                    server.abort(command.header.transaction_id, 0x0504_0001) // Command specifier invalid
                }
                Err(_) => server.abort(command.header.transaction_id, 0x0800_0000), // General error parsing payload
            }
        }
        Segmentation::Segment | Segmentation::Complete => {
            // Take the state to avoid mutable borrow issues when calling write_to_od/abort
            if let SdoServerState::SegmentedDownload(mut transfer_state) =
                core::mem::take(server.sequence_handler.state_mut())
            {
                if transfer_state.transaction_id != command.header.transaction_id {
                    error!(
                        "Mismatched transaction ID during segmented download. Expected {}, got {}",
                        transfer_state.transaction_id, command.header.transaction_id
                    );
                    // Put state back before aborting
                    *server.sequence_handler.state_mut() =
                        SdoServerState::SegmentedDownload(transfer_state);
                    return server.abort(command.header.transaction_id, 0x0800_0000); // General error
                }

                // Delegate processing to the transfer state
                match transfer_state.process_download_segment(&command, od, current_time_us) {
                    Ok(true) => {
                        // Complete and successful
                        *server.sequence_handler.state_mut() = SdoServerState::Established;
                        SdoCommand {
                            header: response_header, // Send final ACK
                            data_size: None,
                            payload: Vec::new(),
                        }
                    }
                    Ok(false) => {
                        // More segments needed
                        *server.sequence_handler.state_mut() =
                            SdoServerState::SegmentedDownload(transfer_state);
                        SdoCommand {
                            header: response_header, // Send ACK for segment
                            data_size: None,
                            payload: Vec::new(),
                        }
                    }
                    Err(abort_code) => {
                        // Abort
                        *server.sequence_handler.state_mut() = SdoServerState::Established;
                        server.abort(command.header.transaction_id, abort_code)
                    }
                }
            } else {
                error!(
                    "Received unexpected SDO segment frame (TID {}). Current state: {:?}",
                    command.header.transaction_id,
                    server.sequence_handler.state()
                );
                // Abort, reset state just in case
                *server.sequence_handler.state_mut() = SdoServerState::Established;
                server.abort(command.header.transaction_id, 0x0504_0003) // Invalid sequence
            }
        }
    }
}

pub(super) fn handle_read_by_name(
    server: &mut SdoServer,
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
                handle_read_by_index(
                    server,
                    read_req_command,
                    response_header,
                    od,
                    current_time_us,
                )
            } else {
                server.abort(command.header.transaction_id, 0x060A_0023) // Resource not available
            }
        }
        Err(PowerlinkError::SdoInvalidCommandPayload) => {
            server.abort(command.header.transaction_id, 0x0504_0001) // Command specifier invalid
        }
        Err(_) => server.abort(command.header.transaction_id, 0x0800_0000), // General error
    }
}

pub(super) fn handle_write_by_name(
    server: &mut SdoServer,
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
                handle_write_by_index(server, command, response_header, od, current_time_us)
            } else {
                server.abort(command.header.transaction_id, 0x060A_0023) // Resource not available
            }
        }
        Err(PowerlinkError::SdoInvalidCommandPayload) => {
            server.abort(command.header.transaction_id, 0x0504_0001) // Command specifier invalid
        }
        Err(_) => server.abort(command.header.transaction_id, 0x0800_0000), // General error
    }
}

pub(super) fn handle_read_all_by_index(
    server: &mut SdoServer,
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
                            return server.abort(command.header.transaction_id, 0x0609_0011); // Sub-index access error
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
                            *server.sequence_handler.state_mut() =
                                SdoServerState::SegmentedUpload(transfer_state);
                        }
                        response_command
                    }
                }
                Some(crate::od::Object::Variable(_)) => {
                    // ReadAllByIndex is not valid for Variables
                    server.abort(command.header.transaction_id, 0x0609_0030) // Value range exceeded (not a record/array)
                }
                None => {
                    // Object itserver doesn't exist
                    server.abort(command.header.transaction_id, 0x0602_0000) // Object does not exist
                }
            }
        }
        Ok(_) => {
            // Sub-index was not 0
            server.abort(command.header.transaction_id, 0x0609_0011) // Sub-index parameter invalid for ReadAll
        }
        Err(PowerlinkError::SdoInvalidCommandPayload) => {
            server.abort(command.header.transaction_id, 0x0504_0001) // Command specifier invalid
        }
        Err(_) => server.abort(command.header.transaction_id, 0x0800_0000), // General error
    }
}

pub(super) fn handle_read_multiple_params(
    server: &mut SdoServer,
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
                        return server.abort(command.header.transaction_id, abort_code);
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
                    *server.sequence_handler.state_mut() =
                        SdoServerState::SegmentedUpload(transfer_state);
                }
                response_command
            }
        }
        Err(PowerlinkError::SdoInvalidCommandPayload) => {
            server.abort(command.header.transaction_id, 0x0504_0001) // Command specifier invalid
        }
        Err(_) => server.abort(command.header.transaction_id, 0x0800_0000), // General error
    }
}

pub(super) fn handle_max_segment_size(
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
    let response_payload = [max_size_client.to_le_bytes(), max_size_server.to_le_bytes()].concat();

    SdoCommand {
        header: response_header,
        data_size: None,
        payload: response_payload,
    }
}
