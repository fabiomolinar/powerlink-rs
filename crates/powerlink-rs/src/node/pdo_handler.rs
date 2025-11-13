use crate::frame::error::{DllError, DllErrorManager, ErrorCounters, ErrorHandler};
use crate::node::NodeContext;
use crate::od::{ObjectValue, constants};
use crate::pdo::{PDOVersion, PdoMappingEntry};
use crate::types::{C_ADR_MN_DEF_NODE_ID, NodeId};
use log::{error, trace, warn};
use alloc::vec::Vec;

/// A trait for handling Process Data Object (PDO) logic.
/// The lifetime parameter 's matches the lifetime of the Node implementing this trait.
pub trait PdoHandler<'s>: NodeContext<'s> {
    /// Provides access to the node's DLL error manager.
    fn dll_error_manager(&mut self) -> &mut DllErrorManager<impl ErrorCounters, impl ErrorHandler>;

    /// Helper to update the PDO error logging objects (0x1C80, 0x1C81).
    fn update_pdo_error_object(&mut self, object_index: u16, source_node_id: NodeId) {
        let node_id_val = source_node_id.0;
        // Do not log for the MN or other invalid/broadcast node IDs.
        if node_id_val == 0 || node_id_val >= C_ADR_MN_DEF_NODE_ID {
            return;
        }

        // Per spec 7.3.1.2.3, the node list format is a 32-byte bitmask.
        // This logic uses a standard bitmask interpretation where:
        // byte_index = (id-1)/8 and bit_index = (id-1)%8.
        let byte_index = (node_id_val - 1) as usize / 8;
        let bit_index = (node_id_val - 1) % 8;

        // Use core().od for immutable read
        if let Some(cow) = self.core().od.read(object_index, 0) {
            if let ObjectValue::OctetString(mut bytes) = cow.into_owned() {
                if let Some(byte_to_modify) = bytes.get_mut(byte_index) {
                    *byte_to_modify |= 1 << bit_index;
                    // Write back to the OD, ignoring access rights.
                    // Use core_mut().od for mutable write
                    if let Err(e) = self.core_mut().od.write_internal(
                        object_index,
                        0,
                        ObjectValue::OctetString(bytes),
                        false,
                    ) {
                        error!(
                            "[PDO] Failed to update PDO error object {:#06X}: {:?}",
                            object_index, e
                        );
                    }
                } else {
                    warn!(
                        "[PDO] Error object {:#06X} has incorrect length (expected 32 bytes).",
                        object_index
                    );
                }
            } else {
                warn!(
                    "[PDO] Error object {:#06X} is not an OctetString.",
                    object_index
                );
            }
        }
        // If the object doesn't exist (it's optional), we silently do nothing.
    }

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
            return;
        }
        trace!(
            "Attempting to consume PDO payload ({} bytes) from Node {}",
            payload.len(),
            source_node_id.0
        );

        // Find the correct mapping for this source node by searching RPDO Comm Params (0x14xx)
        let mut mapping_index_opt = None;

        for i in 0..256 {
            let comm_param_index = constants::IDX_RPDO_COMM_PARAM_REC_START + i as u16;

            // Check if this RPDO channel is configured for the source node
            if let Some(node_id_val) = self
                .core()
                .od
                .read_u8(comm_param_index, constants::SUBIDX_PDO_COMM_PARAM_NODEID_U8)
            {
                // PReq from MN is mapped to NodeID 0 in OD; PRes is mapped to the source CN's ID.
                let matches_source = (source_node_id.0 == C_ADR_MN_DEF_NODE_ID && node_id_val == 0)
                    || (source_node_id.0 != 0 && node_id_val == source_node_id.0);

                if matches_source {
                    // Found the correct communication parameter object
                    let expected_version = self
                        .core()
                        .od
                        .read_u8(
                            comm_param_index,
                            constants::SUBIDX_PDO_COMM_PARAM_VERSION_U8,
                        )
                        .unwrap_or(0);

                    // Check PDO Mapping Version (Spec 6.4.2)
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
                            "PDO version mismatch for source Node {}. Expected {}, got {}. Ignoring payload. [E_PDO_MAP_VERS]",
                            source_node_id.0, expected_version, received_version.0
                        );
                        self.dll_error_manager()
                            .handle_error(DllError::PdoMapVersion {
                                node_id: source_node_id,
                            });
                        self.update_pdo_error_object(
                            constants::IDX_PDO_ERR_MAP_VERS_OSTR,
                            source_node_id,
                        );
                        return;
                    }

                    mapping_index_opt =
                        Some(constants::IDX_RPDO_MAPPING_PARAM_REC_START + i as u16);
                    break;
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
                return;
            }
        };

        // We have a valid mapping, now process it
        if let Some(mapping_cow) = self.core().od.read(mapping_index, 0) {
            if let ObjectValue::Unsigned8(num_entries) = *mapping_cow {
                if num_entries == 0 {
                    return;
                }
                trace!(
                    "Applying RPDO mapping {:#06X} with {} entries for Node {}",
                    mapping_index, num_entries, source_node_id.0
                );
                // Read all mapping entries first, as apply_rpdo_mapping_entry needs &mut self
                let mut mapping_entries = Vec::new();
                for i in 1..=num_entries {
                    if let Some(entry_cow) = self.core().od.read(mapping_index, i) {
                        if let ObjectValue::Unsigned64(raw_mapping) = *entry_cow {
                            mapping_entries.push(PdoMappingEntry::from_u64(raw_mapping));
                        }
                    }
                }

                // Now iterate over the collected entries
                for entry in &mapping_entries {
                    // If any entry fails, stop processing this PDO entirely.
                    if self
                        .apply_rpdo_mapping_entry(entry, payload, source_node_id)
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
    }

    /// Helper for `consume_pdo_payload` to apply a single mapping entry.
    /// Returns a Result to indicate if processing should stop.
    fn apply_rpdo_mapping_entry(
        &mut self,
        entry: &PdoMappingEntry,
        payload: &[u8],
        source_node_id: NodeId,
    ) -> Result<(), ()> {
        let (Some(offset), Some(length)) = (entry.byte_offset(), entry.byte_length()) else {
            warn!(
                "Bit-level RPDO mapping is not supported. Index: 0x{:04X}, SubIndex: {}.",
                entry.index, entry.sub_index
            );
            return Ok(()); // Continue with next entry
        };

        if payload.len() < offset + length {
            warn!(
                "RPDO mapping for 0x{:04X}/{} from Node {} is out of bounds. Payload size: {}, expected at least {}. [E_PDO_SHORT_RX]",
                entry.index,
                entry.sub_index,
                source_node_id.0,
                payload.len(),
                offset + length
            );
            self.dll_error_manager()
                .handle_error(DllError::PdoPayloadShort {
                    node_id: source_node_id,
                });
            self.update_pdo_error_object(constants::IDX_PDO_ERR_SHORT_RX_OSTR, source_node_id);
            return Err(()); // Stop processing this PDO
        }

        let data_slice = &payload[offset..offset + length];

        // --- SDO-in-PDO LOGIC ---
        // Check if this mapping points to an SDO container object
        match entry.index {
            // SDO Server Channel (0x1200 - 0x127F): This is a request for us.
            0x1200..=0x127F => {
                trace!(
                    "[SDO-PDO] Server: Received request on channel {:#06X}",
                    entry.index
                );
                // FIX: Borrow core_mut() once, then access od and server
                let core = self.core_mut();
                core.embedded_sdo_server.handle_request(
                    entry.index,
                    data_slice,
                    &core.od, // Pass immutable reference to OD
                );
                return Ok(()); // SDO handled, skip standard data write
            }
            // SDO Client Channel (0x1280 - 0x12FF): This is a response for us.
            0x1280..=0x12FF => {
                trace!(
                    "[SDO-PDO] Client: Received response on channel {:#06X}",
                    entry.index
                );
                self.core_mut()
                    .embedded_sdo_client
                    .handle_response(entry.index, data_slice);
                return Ok(()); // SDO handled, skip standard data write
            }
            // Standard Data Object
            _ => {
                // Fall through to standard data write logic
            }
        }
        // --- END SDO-in-PDO LOGIC ---

        // Get an immutable reference to the OD first
        let type_template_option = self
            .core()
            .od
            .read(entry.index, entry.sub_index)
            .map(|cow| cow.into_owned());

        let Some(type_template) = type_template_option else {
            warn!(
                "RPDO mapping for 0x{:04X}/{} failed: OD entry not found.",
                entry.index, entry.sub_index
            );
            return Ok(());
        };

        match ObjectValue::deserialize(data_slice, &type_template) {
            Ok(value) => {
                if core::mem::discriminant(&value) != core::mem::discriminant(&type_template) {
                    warn!(
                        "RPDO type mismatch after deserialize for 0x{:04X}/{}. Expected {:?}, got {:?}. Value ignored.",
                        entry.index, entry.sub_index, type_template, value
                    );
                    self.dll_error_manager()
                        .handle_error(DllError::PdoMapVersion {
                            node_id: source_node_id,
                        });
                    return Err(());
                }

                trace!(
                    "Applying RPDO: Writing {:?} to 0x{:04X}/{}",
                    value, entry.index, entry.sub_index
                );
                // Now get a mutable reference to write
                if let Err(e) =
                    self.core_mut()
                        .od
                        .write_internal(entry.index, entry.sub_index, value, false)
                {
                    error!(
                        "Critical Error: Failed to write RPDO data to existing OD entry 0x{:04X}/{}: {:?}",
                        entry.index, entry.sub_index, e
                    );
                }
            }
            Err(e) => {
                warn!(
                    "Failed to deserialize RPDO data for 0x{:04X}/{}: {:?}. Data slice: {:02X?}",
                    entry.index, entry.sub_index, e, data_slice
                );
                self.dll_error_manager()
                    .handle_error(DllError::PdoPayloadShort {
                        node_id: source_node_id,
                    });
                return Err(());
            }
        }
        Ok(())
    }
}
