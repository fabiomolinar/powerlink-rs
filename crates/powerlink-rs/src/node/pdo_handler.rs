// crates/powerlink-rs/src/node/pdo_handler.rs
// Refined PDO consumption: Improved logging, error handling for mapping issues.

use crate::frame::error::{DllError, DllErrorManager, ErrorCounters, ErrorHandler};
use crate::od::{ObjectDictionary, ObjectValue};
use crate::pdo::{PDOVersion, PdoMappingEntry};
use crate::types::NodeId;
use log::{error, trace, warn}; // Added error

const OD_IDX_RPDO_COMM_PARAM_BASE: u16 = 0x1400;
const OD_IDX_RPDO_MAPP_PARAM_BASE: u16 = 0x1600;
const OD_SUBIDX_PDO_COMM_NODEID: u8 = 1;
const OD_SUBIDX_PDO_COMM_VERSION: u8 = 2;

/// A trait for handling Process Data Object (PDO) logic.
/// The lifetime parameter 's matches the lifetime of the Node implementing this trait.
pub trait PdoHandler<'s> {
    /// Provides access to the node's Object Dictionary with the correct lifetime.
    fn od(&mut self) -> &mut ObjectDictionary<'s>;

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
        is_ready: bool, // RD flag from PReq/PRes
    ) {
        if !is_ready {
            trace!(
                "Ignoring PDO payload from Node {}: RD flag is not set.",
                source_node_id.0
            );
            return; // Data is not valid
        }
        trace!(
            "Attempting to consume PDO payload ({} bytes) from Node {}",
            payload.len(),
            source_node_id.0
        );

        // Find the correct mapping for this source node by searching RPDO Comm Params (0x14xx)
        let mut mapping_index_opt = None;
        let mut expected_version = 0u8; // Store expected version if found

        for i in 0..256 {
            // Check all possible RPDO channels
            let comm_param_index = OD_IDX_RPDO_COMM_PARAM_BASE + i as u16;

            // Check if this RPDO channel is configured for the source node
            // Use self.od() which now correctly returns &'s mut
            if let Some(node_id_val) = self
                .od()
                .read_u8(comm_param_index, OD_SUBIDX_PDO_COMM_NODEID)
            {
                // Handle PReq case where NodeID in OD is 0
                let matches_source = (source_node_id.0 == 0 && node_id_val == 0) // PReq mapping
                                  || (source_node_id.0 != 0 && node_id_val == source_node_id.0); // PRes mapping

                if matches_source {
                    // Found the correct communication parameter object
                    expected_version = self
                        .od()
                        .read_u8(comm_param_index, OD_SUBIDX_PDO_COMM_VERSION)
                        .unwrap_or(0); // Default to 0 if not found

                    // Check PDO Mapping Version (Spec 6.4.2)
                    // If expected is 0, received must also be 0.
                    // If expected > 0, received main version must match, received sub >= expected sub.
                    let expected_main = expected_version >> 4;
                    let expected_sub = expected_version & 0x0F;
                    let received_main = received_version.0 >> 4;
                    let received_sub = received_version.0 & 0x0F;

                    let version_ok = (expected_version == 0 && received_version.0 == 0)
                        || (expected_version > 0
                            && expected_main == received_main
                            && received_sub >= expected_sub);

                    if !version_ok {
                        warn!(
                            "PDO version mismatch for source Node {}. Expected {}, got {}. Ignoring payload.",
                            source_node_id.0, expected_version, received_version.0
                        );
                        // Use self.dll_error_manager() - Log error E_PDO_MAP_VERS (Spec 6.4.8.1.1)
                        self.dll_error_manager()
                            .handle_error(DllError::PdoMapVersion {
                                node_id: source_node_id,
                            });
                        return; // Stop processing this payload due to version mismatch
                    }

                    // Version matches, store the corresponding mapping index
                    mapping_index_opt = Some(OD_IDX_RPDO_MAPP_PARAM_BASE + i as u16);
                    break; // Found the mapping, exit the loop
                }
            }
        }

        let mapping_index = match mapping_index_opt {
            Some(index) => index,
            None => {
                trace!(
                    "No RPDO mapping found or configured for source Node {}.",
                    source_node_id.0
                );
                return; // No mapping defined for this source
            }
        };

        // We have a valid mapping, now process it
        // Use self.od() to read number of entries in the mapping object (0x16xx/0)
        if let Some(mapping_cow) = self.od().read(mapping_index, 0) {
            if let ObjectValue::Unsigned8(num_entries) = *mapping_cow {
                if num_entries == 0 {
                    trace!(
                        "RPDO mapping {:#06X} is disabled (0 entries).",
                        mapping_index
                    );
                    return;
                }
                trace!(
                    "Applying RPDO mapping {:#06X} with {} entries for Node {}",
                    mapping_index, num_entries, source_node_id.0
                );
                for i in 1..=num_entries {
                    // Use self.od() to read the specific mapping entry (0x16xx / i)
                    if let Some(entry_cow) = self.od().read(mapping_index, i) {
                        if let ObjectValue::Unsigned64(raw_mapping) = *entry_cow {
                            let entry = PdoMappingEntry::from_u64(raw_mapping);
                            self.apply_rpdo_mapping_entry(&entry, payload, source_node_id);
                        } else {
                            warn!(
                                "RPDO mapping entry {:#06X}/{} is not U64.",
                                mapping_index, i
                            );
                        }
                    } else {
                        warn!(
                            "Could not read RPDO mapping entry {:#06X}/{}.",
                            mapping_index, i
                        );
                    }
                }
            } else {
                warn!(
                    "RPDO mapping object {:#06X} sub-index 0 is not U8.",
                    mapping_index
                );
            }
        } else {
            warn!(
                "RPDO mapping object {:#06X} sub-index 0 not found.",
                mapping_index
            );
        }
    }

    /// Helper for `consume_pdo_payload` to apply a single mapping entry.
    /// Note: This method needs mutable access to self to call od() and dll_error_manager() mutably.
    fn apply_rpdo_mapping_entry(
        &mut self,
        entry: &PdoMappingEntry,
        payload: &[u8],
        source_node_id: NodeId,
    ) {
        // Assuming byte alignment for now
        let (Some(offset), Some(length)) = (entry.byte_offset(), entry.byte_length()) else {
            warn!(
                "Bit-level RPDO mapping is not supported. Index: 0x{:04X}, SubIndex: {}.",
                entry.index, entry.sub_index
            );
            return;
        };

        // Check payload bounds (Spec 6.4.8.1.2 Unexpected End of PDO)
        if payload.len() < offset + length {
            warn!(
                "RPDO mapping for 0x{:04X}/{} from Node {} is out of bounds. Payload size: {}, expected at least {}.",
                entry.index,
                entry.sub_index,
                source_node_id.0,
                payload.len(),
                offset + length
            );
            // Log error E_PDO_SHORT_RX
            self.dll_error_manager()
                .handle_error(DllError::PdoPayloadShort {
                    node_id: source_node_id,
                });
            return; // Ignore this specific mapping entry, but continue with others if any
        }

        let data_slice = &payload[offset..offset + length];
        // Use self.od() to read the template. Need a separate borrow or clone.
        // Cloning is simpler here to avoid complex borrow checker issues with self.od().write_internal later.
        let type_template_option = self
            .od()
            .read(entry.index, entry.sub_index)
            .map(|cow| cow.into_owned()); // Clone the template value

        let Some(type_template) = type_template_option else {
            // OD entry itself doesn't exist, cannot apply mapping.
            // This is a configuration error, but potentially recoverable if other mappings are valid.
            warn!(
                "RPDO mapping for 0x{:04X}/{} failed: OD entry not found.",
                entry.index, entry.sub_index
            );
            // Optionally log a configuration error here? Spec doesn't mandate DLL error.
            return;
        };

        match ObjectValue::deserialize(data_slice, &type_template) {
            Ok(value) => {
                // Check if the received value type matches the OD entry type *exactly*
                // (deserialize might succeed with type coercion, e.g., u8 into u16, which we might not want for PDO)
                // Using core::mem::discriminant for type comparison without PartialEq on ObjectValue itself.
                if core::mem::discriminant(&value) != core::mem::discriminant(&type_template) {
                    warn!(
                        "RPDO type mismatch after deserialize for 0x{:04X}/{}. Expected {:?}, got {:?}. Value ignored.",
                        entry.index, entry.sub_index, type_template, value
                    );
                    // Log a specific PDO type mismatch error?
                    return;
                }

                trace!(
                    "Applying RPDO: Writing {:?} to 0x{:04X}/{}",
                    value, entry.index, entry.sub_index
                );
                // Use self.od() - Write internally, bypassing access checks as this is network input
                if let Err(e) = self
                    .od()
                    .write_internal(entry.index, entry.sub_index, value, false)
                {
                    // This write should ideally not fail if the OD entry exists and types matched.
                    error!(
                        "Critical Error: Failed to write RPDO data to existing OD entry 0x{:04X}/{}: {:?}",
                        entry.index, entry.sub_index, e
                    );
                }
            }
            Err(e) => {
                // Deserialization failed (e.g., wrong length for fixed type)
                warn!(
                    "Failed to deserialize RPDO data for 0x{:04X}/{}: {:?}. Data slice: {:02X?}",
                    entry.index, entry.sub_index, e, data_slice
                );
                // Log a PDO data error? Spec implies ignoring the PDO.
            }
        }
    }
}
