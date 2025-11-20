// crates/powerlink-rs/src/node/mn/config.rs
//! Handles the parsing of MN-specific configuration from the Object Dictionary.
//! This separates static initialization logic from the runtime cycle logic.

use super::state::CnInfo;
use crate::PowerlinkError;
use crate::frame::ServiceId;
use crate::od::{Object, ObjectDictionary, ObjectValue, constants};
use crate::types::NodeId;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use log::{error, info, warn};

/// Parses the MN's OD configuration to build its internal node lists.
/// (Reference: NMT_NodeAssignment_AU32 0x1F81)
pub(crate) fn parse_mn_node_lists(
    od: &ObjectDictionary,
) -> Result<
    (
        BTreeMap<NodeId, CnInfo>,
        Vec<NodeId>,
        Vec<NodeId>,
        Vec<NodeId>,
        BTreeMap<NodeId, u8>,
    ),
    PowerlinkError,
> {
    let mut node_info = BTreeMap::new();
    let mut mandatory_nodes = Vec::new();
    let mut isochronous_nodes = Vec::new();
    let mut async_only_nodes = Vec::new();
    let mut multiplex_assign = BTreeMap::new();

    if let Some(Object::Array(entries)) = od.read_object(constants::IDX_NMT_NODE_ASSIGNMENT_AU32) {
        // Sub-index 0 is NumberOfEntries. Real entries start at 1.
        for (i, entry) in entries.iter().enumerate() {
            let sub_index = i as u8 + 1;
            if let ObjectValue::Unsigned32(assignment) = entry {
                if (assignment & 1) != 0 {
                    // Bit 0: Node exists
                    if let Ok(node_id) = NodeId::try_from(sub_index) {
                        node_info.insert(node_id, CnInfo::default());
                        if (assignment & (1 << 3)) != 0 {
                            // Bit 3: Node is mandatory
                            mandatory_nodes.push(node_id);
                        }
                        if (assignment & (1 << 8)) == 0 {
                            // Bit 8: 0=Isochronous
                            isochronous_nodes.push(node_id);
                            let mux_cycle_no = od
                                .read_u8(constants::IDX_NMT_MULTIPLEX_ASSIGN_REC, node_id.0)
                                .unwrap_or(0);
                            multiplex_assign.insert(node_id, mux_cycle_no);
                        } else {
                            // 1=Async-only
                            async_only_nodes.push(node_id);
                        }
                    }
                }
            }
        }
    } else {
        error!("Failed to read NMT_NodeAssignment_AU32 (0x1F81)");
        return Err(PowerlinkError::ValidationError(
            "Missing 0x1F81 NMT_NodeAssignment_AU32",
        ));
    }

    info!(
        "MN configured to manage {} nodes ({} mandatory, {} isochronous, {} async-only).",
        node_info.len(),
        mandatory_nodes.len(),
        isochronous_nodes.len(),
        async_only_nodes.len(),
    );

    Ok((
        node_info,
        mandatory_nodes,
        isochronous_nodes,
        async_only_nodes,
        multiplex_assign,
    ))
}

/// Parses the NMT Info Publish Configuration (0x1F9E).
/// Returns a map of Multiplex Cycle Number -> ServiceId.
pub(crate) fn parse_publish_config(od: &ObjectDictionary) -> BTreeMap<u8, ServiceId> {
    let mut publish_config = BTreeMap::new();
    if let Some(Object::Array(entries)) = od.read_object(constants::IDX_NMT_PUBLISH_CONFIG_AU32) {
        // Sub-index 0 is NumberOfEntries. Real entries start at 1.
        for (i, entry) in entries.iter().enumerate() {
            let sub_index = i as u8 + 1;
            if let ObjectValue::Unsigned32(config_val) = entry {
                // Spec 7.2.1.1.18: Bits 0-7 = ServiceId, Bits 8-15 = MultiplexedCycle
                let service_id_byte = (config_val & 0xFF) as u8;
                let cycle_num = ((config_val >> 8) & 0xFF) as u8;

                if cycle_num > 0 && service_id_byte > 0 {
                    match ServiceId::try_from(service_id_byte) {
                        Ok(service_id) => {
                            info!(
                                "Configuring NMT Info Service: {:?} for Mux Cycle {}",
                                service_id, cycle_num
                            );
                            publish_config.insert(cycle_num, service_id);
                        }
                        Err(_) => {
                            warn!(
                                "Ignoring invalid ServiceId {:#04x} in 0x1F9E/{}",
                                service_id_byte, sub_index
                            );
                        }
                    }
                }
            }
        }
    } else {
        info!("NMT Publish Config (0x1F9E) not found. NMT Info Services disabled.");
    }
    publish_config
}
