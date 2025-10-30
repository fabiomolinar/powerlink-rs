// crates/powerlink-rs/src/sdo/state.rs
use crate::sdo::command::{CommandId, CommandLayerHeader, SdoCommand, Segmentation};
use crate::od::ObjectDictionary;
use crate::{PowerlinkError, od::ObjectValue};
use alloc::vec::Vec;
use log::{debug, error, info, warn};

// Constants moved from server.rs
const MAX_EXPEDITED_PAYLOAD: usize = 1452;
const OD_IDX_SDO_TIMEOUT: u16 = 0x1300;
const OD_IDX_SDO_RETRIES: u16 = 0x1302;

/// The state of an SDO connection from the server's perspective.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SdoServerState {
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
    pub(super) transaction_id: u8,
    pub(super) total_size: usize,
    pub(super) data_buffer: Vec<u8>,
    // For uploads, this is the offset of the next byte to be sent.
    // For downloads, this tracks bytes received.
    pub(super) offset: usize,
    pub(super) index: u16,
    pub(super) sub_index: u8,
    // New fields for retransmission logic
    pub(super) deadline_us: Option<u64>,
    pub(super) retransmissions_left: u32,
    pub(super) last_sent_segment: Option<SdoCommand>,
}

impl SdoTransferState {
    /// Creates the next SDO command for a segmented upload.
    /// This logic was moved from SdoServer::handle_segmented_upload.
    /// It returns the command to send and a boolean indicating if this is the last segment.
    pub fn get_next_upload_segment(
        &mut self,
        od: &ObjectDictionary,
        current_time_us: u64,
    ) -> (SdoCommand, bool) {
        let mut response_header = CommandLayerHeader {
            transaction_id: self.transaction_id,
            is_response: true,
            is_aborted: false,
            segmentation: Segmentation::Segment, // Default unless first or last
            command_id: CommandId::ReadByIndex,  // Response to a read request
            segment_size: 0,
        };

        let chunk_size = MAX_EXPEDITED_PAYLOAD;
        let remaining = self.total_size.saturating_sub(self.offset);
        let current_chunk_size = chunk_size.min(remaining);
        // Clone the data slice to be sent.
        let chunk = self.data_buffer[self.offset..self.offset + current_chunk_size].to_vec();

        let data_size = if self.offset == 0 {
            // This is the first segment (Initiate)
            info!(
                "Sending Initiate Segmented Upload: total size {}",
                self.total_size
            );
            response_header.segmentation = Segmentation::Initiate;
            Some(self.total_size as u32)
        } else {
            None // Data size only in Initiate frame
        };

        // Update the offset for the *next* segment
        self.offset += current_chunk_size;
        debug!(
            "Sending upload segment: new offset={}, segment size={}",
            self.offset,
            chunk.len()
        );

        let is_last_segment = if self.offset >= self.total_size {
            // This is the last segment
            info!("Segmented upload complete (TID {}).", self.transaction_id);
            response_header.segmentation = Segmentation::Complete;
            // No timeout needed for the last segment
            self.deadline_us = None;
            self.last_sent_segment = None;
            true
        } else {
            // More segments to follow, set up for retransmission
            let timeout_ms = od.read_u32(OD_IDX_SDO_TIMEOUT, 0).unwrap_or(15000) as u64;
            self.deadline_us = Some(current_time_us + timeout_ms * 1000);
            self.retransmissions_left = od.read_u32(OD_IDX_SDO_RETRIES, 0).unwrap_or(2);
            false
        };

        response_header.segment_size = chunk.len() as u16;

        let command = SdoCommand {
            header: response_header,
            data_size,
            payload: chunk,
        };

        // Store the command for potential retransmission *only if* not the last segment
        if !is_last_segment {
            self.last_sent_segment = Some(command.clone());
        }

        (command, is_last_segment)
    }

    /// Processes an incoming segment for a segmented download.
    /// This logic was moved from SdoServer::handle_write_by_index.
    /// Returns `Ok(true)` if the transfer is complete, `Ok(false)` if more segments are needed.
    /// Returns `Err(abort_code)` if the transfer must be aborted.
    pub fn process_download_segment(
        &mut self,
        command: &SdoCommand,
        od: &mut ObjectDictionary,
        current_time_us: u64,
    ) -> Result<bool, u32> {
        // Check for overflow before extending
        if self.offset + command.payload.len() > self.total_size {
            error!(
                "Segmented download overflow detected. Expected total {}, received at least {}",
                self.total_size,
                self.offset + command.payload.len()
            );
            return Err(0x0607_0010); // Length too high
        }

        self.data_buffer.extend_from_slice(&command.payload);
        self.offset += command.payload.len();
        debug!(
            "Received download segment (TID {}): new offset={}",
            self.transaction_id, self.offset
        );

        if command.header.segmentation == Segmentation::Complete {
            info!(
                "Segmented download complete (TID {}), writing to OD.",
                self.transaction_id
            );
            if self.offset != self.total_size {
                error!(
                    "Segmented download size mismatch (TID {}). Expected {}, got {}",
                    self.transaction_id, self.total_size, self.offset
                );
                let abort_code = if self.offset < self.total_size {
                    0x0607_0013 // Length too low
                } else {
                    0x0607_0012 // Length too high
                };
                return Err(abort_code);
            }

            // Perform the write to the OD
            match self.perform_od_write(od) {
                Ok(_) => Ok(true), // Complete
                Err(abort_code) => Err(abort_code),
            }
        } else {
            // Not complete, reset the timeout
            let timeout_ms = od.read_u32(OD_IDX_SDO_TIMEOUT, 0).unwrap_or(15000) as u64;
            self.deadline_us = Some(current_time_us + timeout_ms * 1000);
            Ok(false) // Not complete
        }
    }

    /// Helper to perform the final write to the Object Dictionary.
    /// This logic was moved from SdoServer::write_to_od.
    pub(super) fn perform_od_write(
        &self,
        od: &mut ObjectDictionary,
    ) -> Result<(), u32> {
        info!(
            "Writing {} bytes to OD 0x{:04X}/{}",
            self.data_buffer.len(),
            self.index,
            self.sub_index
        );
        // Get a clone of the template to avoid double mutable borrow
        match od.read(self.index, self.sub_index).map(|cow| cow.into_owned()) {
            Some(type_template) => match ObjectValue::deserialize(&self.data_buffer, &type_template)
            {
                Ok(value) => {
                    // Double-check type compatibility after deserialize, before writing
                    if core::mem::discriminant(&value) != core::mem::discriminant(&type_template) {
                        error!(
                            "Type mismatch after deserialize (write_to_od): Expected {:?}, got {:?} for 0x{:04X}/{}",
                            type_template, value, self.index, self.sub_index
                        );
                        return Err(0x0607_0010); // Type mismatch
                    }
                    match od.write(self.index, self.sub_index, value) {
                        Ok(_) => Ok(()),
                        // Map OD write errors (which use PowerlinkError) to SDO Abort Codes
                        Err(PowerlinkError::StorageError("Object is read-only")) => {
                            Err(0x0601_0002)
                        } // Attempt to write read-only
                        Err(PowerlinkError::TypeMismatch) => Err(0x0607_0010), // Should be caught earlier, but safety check
                        Err(PowerlinkError::ValidationError(_)) => Err(0x0609_0030), // Value range exceeded (e.g., PDO validation)
                        Err(_) => Err(0x0800_0020), // Data cannot be transferred or stored
                    }
                }
                Err(PowerlinkError::BufferTooShort) => Err(0x0607_0013), // Length too low
                Err(_) => Err(0x0607_0010), // Data type mismatch or length error during deserialize
            },
            // Distinguish between Object not found and Sub-index not found
            None if od.read_object(self.index).is_none() => Err(0x0602_0000), // Object does not exist
            None => Err(0x0609_0011), // Sub-index does not exist
        }
    }
}
