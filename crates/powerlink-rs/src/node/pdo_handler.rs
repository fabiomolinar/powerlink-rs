// crates/powerlink-rs/src/node/pdo_handler.rs
use crate::frame::error::{DllError, DllErrorManager, ErrorCounters, ErrorHandler};
use crate::node::NodeContext;
use crate::od::{ObjectValue, constants};
use crate::pdo::{PDOVersion, PdoMappingEntry};
use crate::types::{C_ADR_MN_DEF_NODE_ID, NodeId};
use alloc::vec::Vec;
use log::{error, trace, warn};

// ... [trait definitions remain the same] ...
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
                let core = self.core_mut();
                // Borrow checker workaround: Split borrow of `core` fields
                let embedded_server = &mut core.embedded_sdo_server;
                let od = &mut core.od;

                embedded_server.handle_request(
                    entry.index,
                    data_slice,
                    od, // Pass MUTABLE reference to OD
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::od::{Object, ObjectDictionary, ObjectEntry, ObjectValue};
    use crate::types::NodeId;
    use crate::pdo::PDOVersion;
    use crate::node::{NodeContext, CoreNodeContext};
    use crate::frame::error::{DllErrorManager, LoggingErrorHandler, CnErrorCounters, ErrorHandler, ErrorCounters};
    use crate::nmt::cn_state_machine::CnNmtStateMachine;
    use crate::sdo::{SdoServer, SdoClient, EmbeddedSdoServer, EmbeddedSdoClient};
    use crate::frame::basic::MacAddress;
    use alloc::borrow::Cow;
    use alloc::vec;

    struct TestNode {
        core: CoreNodeContext<'static>,
        dll_error_manager: DllErrorManager<CnErrorCounters, LoggingErrorHandler>,
        nmt_state_machine: CnNmtStateMachine,
    }

    impl<'a> NodeContext<'a> for TestNode {
        fn is_cn(&self) -> bool { true }
        fn core(&self) -> &CoreNodeContext<'a> { unsafe { core::mem::transmute(&self.core) } }
        fn core_mut(&mut self) -> &mut CoreNodeContext<'a> { unsafe { core::mem::transmute(&mut self.core) } }
        fn nmt_state_machine(&self) -> &dyn crate::nmt::NmtStateMachine { &self.nmt_state_machine }
    }

    impl<'a> PdoHandler<'a> for TestNode {
        fn dll_error_manager(&mut self) -> &mut DllErrorManager<impl ErrorCounters, impl ErrorHandler> {
            &mut self.dll_error_manager
        }
    }

    fn setup_node() -> TestNode {
        let mut od = ObjectDictionary::new(None);
        
        // 1. Target Object (0x2000)
        od.insert(0x2000, ObjectEntry {
            object: Object::Record(vec![
                ObjectValue::Unsigned8(0),  
                ObjectValue::Unsigned16(0)  
            ]),
            ..Default::default()
        });
        od.write(0x2000, 1, ObjectValue::Unsigned8(0)).unwrap();
        od.write(0x2000, 2, ObjectValue::Unsigned16(0)).unwrap();

        // 2. RPDO Comm Param (0x1400)
        od.insert(0x1400, ObjectEntry {
            object: Object::Array(vec![
                ObjectValue::Unsigned8(1), // Sub 1: NodeID = 1
                ObjectValue::Unsigned8(0)  // Sub 2: Version = 0
            ]),
            ..Default::default()
        });
        od.write(0x1400, 1, ObjectValue::Unsigned8(1)).unwrap();

        // 3. RPDO Mapping (0x1600)
        od.insert(0x1600, ObjectEntry {
            object: Object::Array(vec![
                ObjectValue::Unsigned64(0), 
                ObjectValue::Unsigned64(0)
            ]),
            ..Default::default()
        });
        
        // CALCULATE MAPPING ENTRIES CORRECTLY
        // Format: Length(48..63) | Offset(32..47) | Res | Sub(16..23) | Index(0..15)
        
        // Entry 1: Index 0x2000, Sub 1, Offset 0, Length 8 bits
        // 0x0008_0000_0001_2000
        let mapping_1: u64 = (8 << 48) | (0 << 32) | (1 << 16) | 0x2000;
        od.write(0x1600, 1, ObjectValue::Unsigned64(mapping_1)).unwrap();

        // Entry 2: Index 0x2000, Sub 2, Offset 8 bits (1 byte), Length 16 bits
        // 0x0010_0008_0002_2000
        let mapping_2: u64 = (16 << 48) | (8 << 32) | (2 << 16) | 0x2000;
        od.write(0x1600, 2, ObjectValue::Unsigned64(mapping_2)).unwrap();

        od.insert(constants::IDX_PDO_ERR_MAP_VERS_OSTR, ObjectEntry { object: Object::Variable(ObjectValue::OctetString(vec![0; 32])), ..Default::default() });
        od.insert(constants::IDX_PDO_ERR_SHORT_RX_OSTR, ObjectEntry { object: Object::Variable(ObjectValue::OctetString(vec![0; 32])), ..Default::default() });

        let core = CoreNodeContext {
            od,
            mac_address: MacAddress::default(),
            sdo_server: SdoServer::new(),
            sdo_client: SdoClient::new(),
            embedded_sdo_server: EmbeddedSdoServer::new(),
            embedded_sdo_client: EmbeddedSdoClient::new(),
        };

        TestNode {
            core,
            dll_error_manager: DllErrorManager::new(CnErrorCounters::new(), LoggingErrorHandler),
            nmt_state_machine: CnNmtStateMachine::new(NodeId(1), Default::default(), 0),
        }
    }

    #[test]
    fn test_consume_valid_pdo() {
        let mut node = setup_node();
        let payload = [0xAA, 0xBB, 0xCC]; // 0xAA (U8), 0xCCBB (U16)
        
        node.consume_pdo_payload(NodeId(1), &payload, PDOVersion(0), true);

        let val_u8 = node.core.od.read(0x2000, 1).unwrap();
        let val_u16 = node.core.od.read(0x2000, 2).unwrap();

        // Assert using match to avoid unwrap panic hiding info
        if let ObjectValue::Unsigned8(v) = val_u8.as_ref() {
            assert_eq!(*v, 0xAA, "PDO write failed for U8. Expected 0xAA, got {:#02X}", v);
        } else {
            panic!("OD 0x2000/1 has wrong type");
        }

        if let ObjectValue::Unsigned16(v) = val_u16.as_ref() {
            assert_eq!(*v, 0xCCBB, "PDO write failed for U16. Expected 0xCCBB, got {:#04X}", v);
        } else {
             panic!("OD 0x2000/2 has wrong type");
        }
    }

    #[test]
    fn test_consume_pdo_too_short() {
        let mut node = setup_node();
        let payload = [0xAA, 0xBB]; 
        
        node.consume_pdo_payload(NodeId(1), &payload, PDOVersion(0), true);

        let val_u16 = node.core.od.read(0x2000, 2).unwrap();
        assert_eq!(val_u16, Cow::Borrowed(&ObjectValue::Unsigned16(0)));
    }

    #[test]
    fn test_consume_pdo_version_mismatch() {
        let mut node = setup_node();
        let payload = [0xAA, 0xBB, 0xCC];
        
        node.consume_pdo_payload(NodeId(1), &payload, PDOVersion(0x99), true);
        
        let val_u8 = node.core.od.read(0x2000, 1).unwrap();
        assert_eq!(val_u8, Cow::Borrowed(&ObjectValue::Unsigned8(0x00)), "Data written despite version mismatch"); 
    }
}
