// crates/powerlink-rs/src/node/mn/payload.rs
//! Contains functions for building frames sent by the Managing Node.

use crate::node::Node; // Import Node trait for nmt_state()
use super::main::ManagingNode;
use crate::common::{NetTime, RelativeTime};
// use crate::frame::basic::MacAddress; // Unused import
use crate::frame::control::{SoAFlags, SocFlags};
use crate::frame::poll::PReqFlags; // Import directly
use crate::frame::{Codec, PReqFrame, RequestedServiceId, SoAFrame, SocFrame};
use crate::node::NodeAction;
use crate::od::{ObjectDictionary, ObjectValue}; // Removed unused 'Object'
use crate::pdo::{PDOVersion, PdoMappingEntry};
use crate::types::{EPLVersion, NodeId};
use crate::PowerlinkError;
use alloc::vec;
use alloc::vec::Vec;
use log::{debug, error, trace, warn, info}; // Added info
// Need CodecHelpers for serialize_eth_header
use crate::frame::codec::CodecHelpers;


// Constants for OD access
const OD_IDX_TPDO_COMM_PARAM_BASE: u16 = 0x1800;
const OD_IDX_TPDO_MAPP_PARAM_BASE: u16 = 0x1A00;
const OD_SUBIDX_PDO_COMM_NODEID: u8 = 1;
const OD_SUBIDX_PDO_COMM_VERSION: u8 = 2;
const OD_IDX_MN_PREQ_PAYLOAD_LIMIT_LIST: u16 = 0x1F8B;

/// Builds and serializes a SoC frame.
pub(super) fn build_soc_frame(node: &ManagingNode) -> NodeAction {
    trace!("[MN] Building SoC frame.");
    // TODO: Get real NetTime and RelativeTime from system clock or PTP
    let net_time = NetTime {
        seconds: 0,
        nanoseconds: 0,
    };
    let relative_time = RelativeTime {
        seconds: 0,
        nanoseconds: 0,
    };
    // TODO: Determine SoC flags (mc, ps) based on current cycle state
    let soc_flags = SocFlags::default();

    let soc_frame = SocFrame::new(node.mac_address, soc_flags, net_time, relative_time);

    let mut buf = vec![0u8; 64]; // Buffer for POWERLINK section (min size)
    // Serialize only the POWERLINK part
    match soc_frame.serialize(&mut buf) {
        Ok(pl_size) => {
            // Serialize returns padded size for min frames
            buf.truncate(pl_size);
            // Prepend Ethernet header
            let mut eth_frame_buf = vec![0u8; 14 + pl_size];
            CodecHelpers::serialize_eth_header(&soc_frame.eth_header, &mut eth_frame_buf);
            eth_frame_buf[14..].copy_from_slice(&buf);
            NodeAction::SendFrame(eth_frame_buf)
        }
        Err(e) => {
            error!("[MN] Failed to serialize SoC frame: {:?}", e);
            NodeAction::NoAction
        }
    }
}


/// Builds and serializes a PReq frame for a specific CN.
pub(super) fn build_preq_frame(node: &mut ManagingNode, target_node_id: NodeId) -> NodeAction {
    trace!("[MN] Building PReq for Node {}.", target_node_id.0);
    // Fetch MAC using pub(super) method
    let mac_addr = node.get_cn_mac_address(target_node_id);
    let Some(dest_mac) = mac_addr else {
        error!(
            "[MN] Cannot build PReq: MAC address for Node {} not found.",
            target_node_id.0
        );
        return NodeAction::NoAction; // Skip polling this node for now
    };

    // Find TPDO mapping for this CN (e.g., channel matching target_node_id)
    let mut pdo_channel = None;
    for i in 0..256 {
        let comm_param_index = OD_IDX_TPDO_COMM_PARAM_BASE + i as u16;
        if node
            .od
            .read_u8(comm_param_index, OD_SUBIDX_PDO_COMM_NODEID)
            == Some(target_node_id.0)
        {
            pdo_channel = Some(i as u8);
            break;
        }
    }

    let payload_result = pdo_channel.map_or(Ok((Vec::new(), PDOVersion(0))), |channel| {
        build_tpdo_payload(&node.od, channel) // Pass OD as immutable reference
    });

    match payload_result {
        Ok((payload, pdo_version)) => {
            // Determine RD flag based on NMT state
            // Use the Node trait method, now in scope
            let rd_flag = node.nmt_state() == crate::nmt::states::NmtState::NmtOperational;
            // TODO: Determine other PReq flags (MS, EA)
            let flags = PReqFlags { // Use imported PReqFlags directly
                rd: rd_flag,
                ..Default::default()
            };

            let preq = PReqFrame::new(
                node.mac_address, // Use pub(super) field
                dest_mac,
                target_node_id,
                flags,
                pdo_version,
                payload,
            );

            let mut buf = vec![0u8; 1500]; // Buffer for POWERLINK section
            // Serialize only the POWERLINK part
            match preq.serialize(&mut buf) {
                Ok(pl_size) => {
                    // Serialize returns the padded size if needed
                    buf.truncate(pl_size);
                    // Need to prepend Ethernet header before sending
                    let mut eth_frame_buf = vec![0u8; 14 + pl_size];
                    CodecHelpers::serialize_eth_header(&preq.eth_header, &mut eth_frame_buf);
                    eth_frame_buf[14..].copy_from_slice(&buf);
                    NodeAction::SendFrame(eth_frame_buf)
                }
                Err(e) => {
                    error!("[MN] Failed to serialize PReq frame: {:?}", e);
                    NodeAction::NoAction
                }
            }
        }
        Err(e) => {
            error!(
                "[MN] Failed to build PReq payload for Node {}: {:?}",
                target_node_id.0, e
            );
            NodeAction::NoAction
        }
    }
}


/// Builds the payload for a TPDO (PReq) frame destined for a specific channel.
/// Takes OD as an immutable reference.
pub(super) fn build_tpdo_payload(
    od: &ObjectDictionary, // Changed to immutable reference
    channel_index: u8,
) -> Result<(Vec<u8>, PDOVersion), PowerlinkError> {
    let comm_param_index = OD_IDX_TPDO_COMM_PARAM_BASE + channel_index as u16;
    let mapping_index = OD_IDX_TPDO_MAPP_PARAM_BASE + channel_index as u16;

    // Get target Node ID from Comm Param to read correct payload limit
    let target_node_id = od
        .read_u8(comm_param_index, OD_SUBIDX_PDO_COMM_NODEID)
        .unwrap_or(0);

    let pdo_version = PDOVersion(
        od.read_u8(comm_param_index, OD_SUBIDX_PDO_COMM_VERSION)
            .unwrap_or(0),
    );
    let payload_limit = od
        .read_u16(OD_IDX_MN_PREQ_PAYLOAD_LIMIT_LIST, target_node_id)
        .unwrap_or(0) as usize;

    let mut payload = vec![0u8; payload_limit.min(1490)];
    let mut max_offset_len = 0;

    // Read mapping entries from 0x1Axx
    if let Some(mapping_cow) = od.read(mapping_index, 0) {
        if let ObjectValue::Unsigned8(num_entries) = *mapping_cow {
            for i in 1..=num_entries {
                let Some(entry_cow) = od.read(mapping_index, i) else {
                    continue;
                };
                let ObjectValue::Unsigned64(raw_mapping) = *entry_cow else {
                    continue;
                };
                let entry = PdoMappingEntry::from_u64(raw_mapping);

                let (Some(offset), Some(length)) = (entry.byte_offset(), entry.byte_length())
                else {
                    warn!(
                        "Bit-level TPDO mapping not supported for PReq 0x{:04X}/{}",
                        entry.index, entry.sub_index
                    );
                    continue;
                };

                let end_pos = offset + length;
                if end_pos > payload.len() {
                    warn!(
                        "TPDO mapping for PReq 0x{:04X}/{} exceeds PReq payload limit {}. Required: {} bytes.",
                        entry.index, entry.sub_index, payload.len(), end_pos
                    );
                    return Err(PowerlinkError::ValidationError(
                        "PDO mapping exceeds payload limit",
                    ));
                }
                max_offset_len = max_offset_len.max(end_pos);

                // Read value from OD to put into PReq payload
                let Some(value_cow) = od.read(entry.index, entry.sub_index) else {
                    warn!(
                        "TPDO mapping for PReq 0x{:04X}/{} failed: OD entry not found.",
                        entry.index, entry.sub_index
                    );
                    payload[offset..end_pos].fill(0);
                    continue;
                };

                let serialized_data = value_cow.serialize();
                if serialized_data.len() != length {
                    warn!(
                        "TPDO mapping for PReq 0x{:04X}/{} length mismatch. Mapped: {} bytes, Object: {} bytes.",
                        entry.index, entry.sub_index, length, serialized_data.len()
                    );
                    let copy_len = serialized_data.len().min(length);
                    payload[offset..offset + copy_len]
                        .copy_from_slice(&serialized_data[..copy_len]);
                } else {
                    payload[offset..end_pos].copy_from_slice(&serialized_data);
                }
                trace!(
                    "Applied TPDO to PReq: Read {:?} from 0x{:04X}/{}",
                    value_cow, entry.index, entry.sub_index
                );
            }
        }
    }

    payload.truncate(max_offset_len);
    Ok((payload, pdo_version))
}

/// Builds and serializes an SoA frame, potentially granting an async slot.
pub(super) fn build_soa_frame(node: &mut ManagingNode) -> NodeAction {
    trace!("[MN] Building SoA frame.");
    // Read actual EPLVersion from OD (0x1F83)
    let epl_version = EPLVersion(node.od.read_u8(0x1F83, 0).unwrap_or(0x15)); // Default to 1.5

    // --- Basic Async Scheduling ---
    let (req_service, target_node) =
        if let Some(request) = node.async_request_queue.pop_front() { // Access pub(super) field
            // TODO: Differentiate based on priority or request type (NMT vs Generic)
            info!( // Use imported macro
                "[MN] Granting async slot to Node {} (PR={})",
                request.node_id.0, request.priority // Access pub(super) fields
            );
            node.current_phase = super::main::CyclePhase::AsynchronousSoA; // Access pub(super) field & enum
            // TODO: Schedule ASnd timeout
            (RequestedServiceId::UnspecifiedInvite, request.node_id) // Access pub(super) field
        } else {
            node.current_phase = super::main::CyclePhase::Idle; // Access pub(super) field & enum
            (RequestedServiceId::NoService, NodeId(0)) // No requests pending
        };
    // TODO: Handle IdentRequest and StatusRequest scheduling

    let soa_frame = SoAFrame::new(
        node.mac_address, // Access pub(super) field
        node.nmt_state(), // Use trait method
        SoAFlags::default(), // TODO: Set EA/ER flags based on error state
        req_service,
        target_node,
        epl_version,
    );
    let mut buf = vec![0u8; 64]; // Buffer for POWERLINK section
    // Serialize only the POWERLINK part
    match soa_frame.serialize(&mut buf) {
        Ok(pl_size) => {
             // Serialize returns padded size
             buf.truncate(pl_size);
             // Need to prepend Ethernet header before sending
             let mut eth_frame_buf = vec![0u8; 14 + pl_size];
             CodecHelpers::serialize_eth_header(&soa_frame.eth_header, &mut eth_frame_buf);
             eth_frame_buf[14..].copy_from_slice(&buf);
             NodeAction::SendFrame(eth_frame_buf)
        }
        Err(e) => {
            error!("[MN] Failed to serialize SoA frame: {:?}", e);
            NodeAction::NoAction
        }
    }
}


/// Builds and serializes an SoA(IdentRequest) frame.
pub(super) fn build_soa_ident_request(node: &ManagingNode, target_node_id: NodeId) -> NodeAction {
    debug!(
        "[MN] Building SoA(IdentRequest) for Node {}",
        target_node_id.0
    );
    // Read actual EPLVersion from OD (0x1F83)
    let epl_version = EPLVersion(node.od.read_u8(0x1F83, 0).unwrap_or(0x15)); // Default to 1.5 if not found

    let req_service = if target_node_id.0 == 0 {
        RequestedServiceId::NoService // 0 indicates no specific target
    } else {
        RequestedServiceId::IdentRequest
    };

    let soa_frame = SoAFrame::new(
        node.mac_address, // Access pub(super) field
        node.nmt_state(), // Use trait method
        SoAFlags::default(),
        req_service,
        target_node_id,
        epl_version,
    );
    let mut buf = vec![0u8; 64]; // Buffer for POWERLINK section
    // Serialize only the POWERLINK part
    match soa_frame.serialize(&mut buf) {
        Ok(pl_size) => {
            // Serialize returns padded size
            buf.truncate(pl_size);
            // Need to prepend Ethernet header before sending
            let mut eth_frame_buf = vec![0u8; 14 + pl_size];
            CodecHelpers::serialize_eth_header(&soa_frame.eth_header, &mut eth_frame_buf);
            eth_frame_buf[14..].copy_from_slice(&buf);
            NodeAction::SendFrame(eth_frame_buf)
        }
        Err(e) => {
            error!("[MN] Failed to serialize SoA(IdentRequest) frame: {:?}", e);
            NodeAction::NoAction
        }
    }
}

