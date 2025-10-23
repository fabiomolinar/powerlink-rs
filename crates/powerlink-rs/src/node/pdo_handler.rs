// crates/powerlink-rs/src/node/pdo_handler.rs

use crate::frame::error::{DllError, DllErrorManager, ErrorCounters, ErrorHandler}; // Added imports
use crate::od::{ObjectDictionary, ObjectValue};
use crate::pdo::{PdoMappingEntry, PDOVersion};
use crate::types::NodeId;
use log::{trace, warn};

// Constants remain the same...
const OD_IDX_RPDO_COMM_PARAM_BASE: u16 = 0x1400;
const OD_IDX_RPDO_MAPP_PARAM_BASE: u16 = 0x1600;
const OD_SUBIDX_PDO_COMM_NODEID: u8 = 1;
const OD_SUBIDX_PDO_COMM_VERSION: u8 = 2;


/// A trait for handling Process Data Object (PDO) logic.
/// The lifetime parameter 's matches the lifetime of the Node implementing this trait.
pub trait PdoHandler<'s> { // Added lifetime 's
    /// Provides access to the node's Object Dictionary with the correct lifetime.
    fn od(&mut self) -> &mut ObjectDictionary<'s>; // Use lifetime 's

    /// Provides access to the node's DLL error manager.
    fn dll_error_manager(&mut self) -> &mut DllErrorManager<impl ErrorCounters, impl ErrorHandler>;

    /// Reads RPDO mappings for a given source Node ID and writes
    /// data from the payload into the Object Dictionary.
    /// This is a default implementation shared between CN and MN.
    fn consume_pdo_payload(
        &mut self,
        source_node_id: NodeId,
        payload: &[u8],
        received_version: PDOVersion,
        is_ready: bool,
    ) {
        if !is_ready {
            trace!(
                "Ignoring PDO payload from Node {}: RD flag is not set.",
                source_node_id.0
            );
            return; // Data is not valid
        }

        // Find the correct mapping for this source node
        let mut mapping_index = None;
        for i in 0..256 {
            let comm_param_index = OD_IDX_RPDO_COMM_PARAM_BASE + i as u16;
            // Use self.od() which now correctly returns &'s mut
            if let Some(node_id_val) =
                self.od()
                    .read_u8(comm_param_index, OD_SUBIDX_PDO_COMM_NODEID)
            {
                if node_id_val == source_node_id.0 {
                    // Found the correct communication parameter object
                    let expected_version = self
                        .od()
                        .read_u8(comm_param_index, OD_SUBIDX_PDO_COMM_VERSION)
                        .unwrap_or(0);

                    if expected_version != 0 && received_version.0 != expected_version {
                        warn!(
                            "PDO version mismatch for source Node {}. Expected {}, got {}. Ignoring payload.",
                            source_node_id.0, expected_version, received_version.0
                        );
                        // Use self.dll_error_manager()
                        self.dll_error_manager()
                            .handle_error(DllError::PdoMapVersion {
                                node_id: source_node_id,
                            });
                        return;
                    }
                    mapping_index = Some(OD_IDX_RPDO_MAPP_PARAM_BASE + i as u16);
                    break;
                }
            }
        }

        let mapping_index = match mapping_index {
            Some(index) => index,
            None => {
                trace!("No RPDO mapping found for source Node {}.", source_node_id.0);
                return;
            }
        };

        // We have a valid mapping, now process it
        // Use self.od()
        if let Some(mapping_cow) = self.od().read(mapping_index, 0) {
            if let ObjectValue::Unsigned8(num_entries) = *mapping_cow {
                for i in 1..=num_entries {
                     // Use self.od()
                    if let Some(entry_cow) = self.od().read(mapping_index, i) {
                        if let ObjectValue::Unsigned64(raw_mapping) = *entry_cow {
                            let entry = PdoMappingEntry::from_u64(raw_mapping);
                            self.apply_rpdo_mapping_entry(&entry, payload, source_node_id);
                        }
                    }
                }
            }
        }
    }

    /// Helper for `consume_pdo_payload` to apply a single mapping entry.
    /// Note: This method needs mutable access to self to call od() and dll_error_manager() mutably.
    fn apply_rpdo_mapping_entry(
        &mut self, // Changed to &mut self
        entry: &PdoMappingEntry,
        payload: &[u8],
        source_node_id: NodeId,
    ) {
        // Get byte-aligned offset and length
        let (Some(offset), Some(length)) = (entry.byte_offset(), entry.byte_length()) else {
            warn!("Bit-level PDO mapping is not supported. Index: {}, SubIndex: {}.", entry.index, entry.sub_index);
            return;
        };

        // Check payload bounds
        if payload.len() < offset + length {
            warn!(
                "RPDO mapping for 0x{:04X}/{} from Node {} is out of bounds. Payload size: {}, expected at least {}.",
                entry.index, entry.sub_index, source_node_id.0, payload.len(), offset + length
            );
             // Use self.dll_error_manager()
            self.dll_error_manager()
                .handle_error(DllError::PdoPayloadShort {
                    node_id: source_node_id,
                });
            return;
        }

        let data_slice = &payload[offset..offset + length];
        // Use self.od() to read the template. Need a separate borrow or clone.
        // Cloning is simpler here to avoid complex borrow checker issues with self.od().write_internal later.
        let type_template_option = self.od().read(entry.index, entry.sub_index).map(|cow| cow.into_owned());

        let Some(type_template) = type_template_option else {
             warn!("RPDO mapping for 0x{:04X}/{} failed: OD entry not found.", entry.index, entry.sub_index);
             return;
        };

        match ObjectValue::deserialize(data_slice, &type_template) {
            Ok(value) => {
                trace!(
                    "Applying RPDO: Writing {:?} to 0x{:04X}/{}",
                    value,
                    entry.index,
                    entry.sub_index
                );
                 // Use self.od()
                if let Err(e) =
                    self.od()
                        .write_internal(entry.index, entry.sub_index, value, false)
                {
                    warn!(
                        "Failed to write RPDO data to 0x{:04X}/{}: {:?}",
                        entry.index, entry.sub_index, e
                    );
                }
            }
            Err(e) => {
                warn!(
                    "Failed to deserialize RPDO data for 0x{:04X}/{}: {:?}",
                    entry.index, entry.sub_index, e
                );
            }
        }
    }
}