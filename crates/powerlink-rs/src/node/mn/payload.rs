// crates/powerlink-rs/src/node/mn/payload.rs
use super::scheduler;
use super::state::MnContext;
use crate::PowerlinkError;
use crate::common::{NetTime, RelativeTime};
use crate::frame::basic::MacAddress;
use crate::frame::control::{SoAFlags, SocFlags};
use crate::frame::poll::PReqFlags;
use crate::frame::{
    ASndFrame, PReqFrame, PowerlinkFrame, RequestedServiceId, ServiceId, SoAFrame, SocFrame,
};
use crate::nmt::NmtStateMachine;
use crate::nmt::events::MnNmtCommandRequest;
use crate::od::ObjectValue;
use crate::pdo::{PDOVersion, PdoMappingEntry};
use crate::sdo::asnd::serialize_sdo_asnd_payload;
use crate::sdo::command::SdoCommand;
use crate::sdo::sequence::SequenceLayerHeader;
use crate::types::{C_ADR_BROADCAST_NODE_ID, C_ADR_MN_DEF_NODE_ID, EPLVersion, NodeId};
use alloc::vec;
use alloc::vec::Vec;
use log::{debug, error, trace, warn};

// Import NmtCommandData
use crate::nmt::states::NmtState;
use crate::node::mn::state::{CnState, NmtCommandData};

// Constants for OD access
const OD_IDX_TPDO_COMM_PARAM_BASE: u16 = 0x1800;
const OD_IDX_TPDO_MAPP_PARAM_BASE: u16 = 0x1A00;
const OD_SUBIDX_PDO_COMM_NODEID: u8 = 1;
const OD_SUBIDX_PDO_COMM_VERSION: u8 = 2;
const OD_IDX_MN_PREQ_PAYLOAD_LIMIT_LIST: u16 = 0x1F8B;
const OD_IDX_EPL_VERSION: u16 = 0x1F83;

/// Builds a SoC frame.
pub(super) fn build_soc_frame(
    context: &MnContext,
    current_multiplex_cycle: u8,
    multiplex_cycle_len: u8,
) -> PowerlinkFrame {
    trace!("[MN] Building SoC frame.");
    // TODO: Get real NetTime and RelativeTime from system clock or PTP
    let net_time = NetTime {
        seconds: (context.current_cycle_start_time_us / 1_000_000) as u32,
        nanoseconds: ((context.current_cycle_start_time_us % 1_000_000) * 1000) as u32,
    };
    let relative_time = RelativeTime {
        seconds: 0,
        nanoseconds: 0,
    };

    // MC flag is toggled when the *last* multiplexed cycle has *ended*
    let mc_flag = multiplex_cycle_len > 0 && current_multiplex_cycle == 0;
    // TODO: Implement PS flag logic based on Prescaler (OD 0x1F98/9)
    let ps_flag = false;
    let soc_flags = SocFlags {
        mc: mc_flag,
        ps: ps_flag,
    };

    PowerlinkFrame::Soc(SocFrame::new(
        context.core.mac_address,
        soc_flags,
        net_time,
        relative_time,
    ))
}

/// Builds a PReq frame for a specific CN.
pub(super) fn build_preq_frame(
    context: &mut MnContext,
    target_node_id: NodeId,
    is_multiplexed: bool,
) -> PowerlinkFrame {
    trace!("[MN] Building PReq for Node {}.", target_node_id.0);
    let mac_addr = scheduler::get_cn_mac_address(context, target_node_id);
    let Some(dest_mac) = mac_addr else {
        error!(
            "[MN] Cannot build PReq: MAC address for Node {} not found.",
            target_node_id.0
        );
        // Return a dummy frame that will fail serialization if used
        return PowerlinkFrame::Soc(SocFrame::new(
            context.core.mac_address,
            Default::default(),
            NetTime {
                seconds: 0,
                nanoseconds: 0,
            },
            RelativeTime {
                seconds: 0,
                nanoseconds: 0,
            },
        ));
    };

    // Find the TPDO channel configured for this target CN.
    let mut pdo_channel = None;
    for i in 0..256 {
        let comm_param_index = OD_IDX_TPDO_COMM_PARAM_BASE + i as u16;
        if context
            .core
            .od
            .read_u8(comm_param_index, OD_SUBIDX_PDO_COMM_NODEID)
            == Some(target_node_id.0)
        {
            pdo_channel = Some(i as u8);
            break;
        }
    }

    // Pass the mutable context to build_tpdo_payload
    let payload_result = pdo_channel.map_or(Ok((Vec::new(), PDOVersion(0))), |channel| {
        build_tpdo_payload(context, channel)
    });

    match payload_result {
        Ok((payload, pdo_version)) => {
            let rd_flag = context.nmt_state_machine.current_state()
                == crate::nmt::states::NmtState::NmtOperational;
            // Get the last known EA flag for this node
            let ea_flag = context
                .node_info
                .get(&target_node_id)
                .is_some_and(|info| info.ea_flag);

            let flags = PReqFlags {
                rd: rd_flag,
                ms: is_multiplexed,
                ea: ea_flag,
            };

            PowerlinkFrame::PReq(PReqFrame::new(
                context.core.mac_address,
                dest_mac,
                target_node_id,
                flags,
                pdo_version,
                payload,
            ))
        }
        Err(e) => {
            error!(
                "[MN] Failed to build PReq payload for Node {}: {:?}",
                target_node_id.0, e
            );
            // Return a dummy frame
            PowerlinkFrame::Soc(SocFrame::new(
                context.core.mac_address,
                Default::default(),
                NetTime {
                    seconds: 0,
                    nanoseconds: 0,
                },
                RelativeTime {
                    seconds: 0,
                    nanoseconds: 0,
                },
            ))
        }
    }
}

/// Builds the payload for a TPDO (in this case, for a PReq frame).
pub(super) fn build_tpdo_payload(
    context: &mut MnContext,
    channel_index: u8,
) -> Result<(Vec<u8>, PDOVersion), PowerlinkError> {
    let comm_param_index = OD_IDX_TPDO_COMM_PARAM_BASE + channel_index as u16;
    let mapping_index = OD_IDX_TPDO_MAPP_PARAM_BASE + channel_index as u16;
    let od = &context.core.od;

    let target_node_id = od
        .read_u8(comm_param_index, OD_SUBIDX_PDO_COMM_NODEID)
        .unwrap_or(0);
    let pdo_version = PDOVersion(
        od.read_u8(comm_param_index, OD_SUBIDX_PDO_COMM_VERSION)
            .unwrap_or(0),
    );

    let payload_limit = od
        .read_u16(OD_IDX_MN_PREQ_PAYLOAD_LIMIT_LIST, target_node_id)
        .unwrap_or(36) as usize;
    let payload_limit = payload_limit.min(crate::types::C_DLL_ISOCHR_MAX_PAYL as usize);

    let mut payload = vec![0u8; payload_limit];

    if let Some(ObjectValue::Unsigned8(num_entries)) = od.read(mapping_index, 0).as_deref() {
        if *num_entries > 0 {
            trace!(
                "Building MN TPDO for channel {} with {} entries.",
                channel_index, num_entries
            );
            for i in 1..=*num_entries {
                let Some(entry_cow) = od.read(mapping_index, i) else {
                    warn!(
                        "[MN] Could not read mapping entry {} for TPDO channel {}",
                        i, channel_index
                    );
                    continue;
                };
                let ObjectValue::Unsigned64(raw_mapping) = *entry_cow else {
                    warn!(
                        "[MN] Mapping entry {} for TPDO channel {} is not U64",
                        i, channel_index
                    );
                    continue;
                };

                let entry = PdoMappingEntry::from_u64(raw_mapping);
                let (Some(offset), Some(length)) = (entry.byte_offset(), entry.byte_length())
                else {
                    warn!(
                        "[MN] Bit-level TPDO mapping not supported for PReq 0x{:04X}/{}",
                        entry.index, entry.sub_index
                    );
                    continue;
                };

                let end_pos = offset + length;
                if end_pos > payload_limit {
                    error!(
                        "[MN] TPDO mapping for PReq exceeds payload limit for Node {}. [E_PDO_MAP_OVERRUN]",
                        target_node_id
                    );
                    return Err(PowerlinkError::PdoMapOverrun);
                }

                let data_slice = &mut payload[offset..end_pos];

                // --- SDO-in-PDO LOGIC ---
                match entry.index {
                    // SDO Server Channel (0x1200 - 0x127F): Container for a response from the MN.
                    0x1200..=0x127F => {
                        trace!(
                            "[SDO-PDO] MN Server: Building response for TPDO channel {:#06X}",
                            entry.index
                        );
                        let response_payload = context
                            .core
                            .embedded_sdo_server
                            .get_pending_response(entry.index, length);
                        data_slice.copy_from_slice(&response_payload);
                    }
                    // SDO Client Channel (0x1280 - 0x12FF): Container for a request from the MN.
                    0x1280..=0x12FF => {
                        trace!(
                            "[SDO-PDO] MN Client: Building request for TPDO channel {:#06X}",
                            entry.index
                        );
                        let request_payload = context
                            .core
                            .embedded_sdo_client
                            .get_pending_request(entry.index, length);
                        data_slice.copy_from_slice(&request_payload);
                    }
                    // Standard Data Object
                    _ => {
                        // --- This is the OLD logic, now in the 'else' branch ---
                        let Some(value_cow) = od.read(entry.index, entry.sub_index) else {
                            warn!(
                                "[MN] TPDO mapping for PReq 0x{:04X}/{} failed: OD entry not found. Filling with zeros.",
                                entry.index, entry.sub_index
                            );
                            // data_slice is already zeros, so just continue
                            continue;
                        };
                        let serialized_data = value_cow.serialize();
                        if serialized_data.len() != length {
                            warn!(
                                "[MN] TPDO mapping for PReq 0x{:04X}/{} length mismatch. Mapped: {} bytes, Object: {} bytes.",
                                entry.index,
                                entry.sub_index,
                                length,
                                serialized_data.len()
                            );
                            let copy_len = serialized_data.len().min(length);
                            data_slice[..copy_len].copy_from_slice(&serialized_data[..copy_len]);
                        } else {
                            data_slice.copy_from_slice(&serialized_data);
                        }
                    }
                }
                // --- END SDO-in-PDO LOGIC ---
            }
        }
    } else {
        warn!(
            "[MN] TPDO Mapping object {:#06X} not found or is invalid.",
            mapping_index
        );
    }

    // The payload size is fixed by payload_limit, not by the last mapped object.
    // Do not truncate the payload.
    Ok((payload, pdo_version))
}

/// Builds an SoA frame based on the scheduled action.
pub(super) fn build_soa_frame(
    context: &MnContext,
    req_service: RequestedServiceId,
    target_node: NodeId,
    set_er_flag: bool,
) -> PowerlinkFrame {
    trace!(
        "[MN] Building SoA frame with service {:?} for Node {}",
        req_service, target_node.0
    );
    let epl_version = EPLVersion(
        context
            .core
            .od
            .read_u8(OD_IDX_EPL_VERSION, 0)
            .unwrap_or(0x15),
    );

    let mut flags = SoAFlags::default();
    flags.er = set_er_flag;

    PowerlinkFrame::SoA(SoAFrame::new(
        context.core.mac_address,
        context.nmt_state_machine.current_state(),
        flags,
        req_service,
        target_node,
        epl_version,
    ))
}

/// Builds an ASnd(NMT Command) frame.
pub(super) fn build_nmt_command_frame(
    context: &MnContext,
    command: MnNmtCommandRequest,
    target_node_id: NodeId,
    command_data: NmtCommandData,
) -> PowerlinkFrame {
    debug!(
        "[MN] Building ASnd(NMT Command={:?}) for Node {}",
        command, target_node_id.0
    );
    let is_broadcast = target_node_id.0 == C_ADR_BROADCAST_NODE_ID;
    let dest_mac = if is_broadcast {
        MacAddress(crate::types::C_DLL_MULTICAST_ASND)
    } else {
        let Some(mac) = scheduler::get_cn_mac_address(context, target_node_id) else {
            error!(
                "[MN] Cannot build NMT Command: MAC for Node {} not found.",
                target_node_id.0
            );
            // Return a dummy frame that will fail serialization
            return PowerlinkFrame::Soc(SocFrame::new(
                context.core.mac_address,
                Default::default(),
                NetTime {
                    seconds: 0,
                    nanoseconds: 0,
                },
                RelativeTime {
                    seconds: 0,
                    nanoseconds: 0,
                },
            ));
        };
        mac
    };

    // --- Build NMT Command Payload ---
    // (Reference: EPSG DS 301, Section 7.3.1.2, Table 123)
    let nmt_payload = match command_data {
        NmtCommandData::None => {
            // Plain NMT State Command (2 bytes: CommandID + Reserved)
            vec![command.as_u8(), 0u8]
        }
        NmtCommandData::HostName(hostname) => {
            // NMTNetHostNameSet (34 bytes: CommandID + Reserved + HostName[32])
            // (Reference: EPSG DS 301, Section 7.3.2.1.1, Table 130)
            let mut payload = Vec::with_capacity(34);
            payload.push(command.as_u8());
            payload.push(0u8);
            let hostname_bytes = hostname.as_bytes();
            let len = hostname_bytes.len().min(32);
            payload.extend_from_slice(&hostname_bytes[..len]);
            payload.resize(34, 0u8);
            payload
        }
        NmtCommandData::FlushArp(flush_target_node) => {
            vec![command.as_u8(), 0u8, flush_target_node.0]
        }
    };
    // --- End of Payload Build ---

    PowerlinkFrame::ASnd(ASndFrame::new(
        context.core.mac_address,
        dest_mac,
        target_node_id,
        NodeId(C_ADR_MN_DEF_NODE_ID),
        ServiceId::NmtCommand,
        nmt_payload,
    ))
}

/// Builds an ASnd(NMT Info) broadcast frame.
/// (Reference: EPSG DS 301, Section 7.3.4)
pub(super) fn build_nmt_info_frame(context: &MnContext, service_id: ServiceId) -> PowerlinkFrame {
    debug!("[MN] Building ASnd(NMT Info={:?}) broadcast.", service_id);

    let nmt_payload = match service_id {
        ServiceId::NMTPublishTime => build_publish_time_payload(context),
        ServiceId::NMTPublishNMTState => vec![context.nmt_state_machine.current_state() as u8],
        ServiceId::NMTPublishNodeState => build_node_state_payload(context),
        ServiceId::NMTPublishNodeList => {
            // Configured CNs (Spec 7.3.4.4.1)
            build_node_list_payload(context, |info| info.state >= CnState::Identified)
        }
        ServiceId::NMTPublishActiveNodes => {
            // Active CNs (Spec 7.3.4.7.1) -> Must be Operational
            build_node_list_payload(context, |info| info.state == CnState::Operational)
        }
        ServiceId::NMTPublishEmergNew => {
            // Nodes with EN flag set (Spec 7.3.4.1.9)
            build_node_list_payload(context, |info| info.en_flag)
        }
        ServiceId::NMTPublishHeartbeat => {
            // TODO: Implement tracking of heartbeat events
            warn!("[MN] NMTPublishHeartbeat not fully implemented. Sending empty node list.");
            vec![0u8; 32]
        }
        _ => {
            error!(
                "[MN] Invalid call to build_nmt_info_frame with ServiceId {:?}",
                service_id
            );
            Vec::new()
        }
    };

    PowerlinkFrame::ASnd(ASndFrame::new(
        context.core.mac_address,
        MacAddress(crate::types::C_DLL_MULTICAST_ASND),
        NodeId(C_ADR_BROADCAST_NODE_ID),
        NodeId(C_ADR_MN_DEF_NODE_ID),
        service_id,
        nmt_payload,
    ))
}

/// Helper for NMTPublishTime
fn build_publish_time_payload(context: &MnContext) -> Vec<u8> {
    let net_time = NetTime {
        seconds: (context.current_cycle_start_time_us / 1_000_000) as u32,
        nanoseconds: ((context.current_cycle_start_time_us % 1_000_000) * 1000) as u32,
    };
    let mut payload = Vec::with_capacity(8);
    payload.extend_from_slice(&net_time.seconds.to_le_bytes());
    payload.extend_from_slice(&net_time.nanoseconds.to_le_bytes());
    payload
}

/// Helper for NMTPublishNodeState
fn build_node_state_payload(context: &MnContext) -> Vec<u8> {
    let mut payload = vec![0u8; 239];
    for (node_id, info) in &context.node_info {
        let idx = (node_id.0 - 1) as usize;
        if idx < payload.len() {
            payload[idx] = info.nmt_state as u8;
        }
    }
    payload
}

/// Generic Helper to build a POWERLINK Node List payload (32 bytes).
///
/// Iterates through all tracked nodes and sets the bit if the `filter` predicate returns true.
/// (Reference: EPSG DS 301, 7.3.1.2.3)
fn build_node_list_payload<F>(context: &MnContext, filter: F) -> Vec<u8>
where
    F: Fn(&super::state::CnInfo) -> bool,
{
    let mut payload_bytes = [0u8; 32];

    let mut set_bit = |node_id: u8| {
        if node_id == 0 || node_id > 254 {
            return;
        }
        let node_idx_0_based = (node_id - 1) as usize;
        let byte_index = node_idx_0_based / 8;
        let bit_index = node_idx_0_based % 8;
        if let Some(byte) = payload_bytes.get_mut(byte_index) {
            *byte |= 1 << bit_index;
        }
    };

    // Check all CNs
    for (node_id, info) in &context.node_info {
        if filter(info) {
            set_bit(node_id.0);
        }
    }

    // Check if MN itself should be included (Node 240)
    // This is context-dependent, but generally, if the MN is part of the active set, include it.
    // For EmergNew, MN doesn't have an EN flag in the same way, but could trigger emergency.
    // For now, we assume MN is active if in a cyclic state.
    let mn_state = context.nmt_state_machine.current_state();
    if mn_state >= NmtState::NmtPreOperational1 {
        // For ActiveNodes/ConfiguredNodes, usually include MN.
        // For EmergNew, only if MN has an emergency (not tracked in node_info).
        // Simplification: Only include MN for "Active/Configured" checks, not status checks.
        // This is inferred by checking if the filter likely targets status flags (like EN).
        // A better approach would be to pass an explicit flag, but we can infer from the
        // state of CNs. Since we don't store MnInfo in node_info, we skip MN for dynamic flags.
        if filter(&super::state::CnInfo {
            state: CnState::Operational, // Dummy success state
            ..Default::default()
        }) {
            set_bit(context.nmt_state_machine.node_id().0);
        }
    }

    payload_bytes.to_vec()
}

/// Builds an ASnd(SDO Request) frame for the SdoClientManager.
pub(super) fn build_sdo_asnd_request(
    context: &MnContext,
    target_node_id: NodeId,
    seq_header: SequenceLayerHeader,
    cmd: SdoCommand,
) -> Result<PowerlinkFrame, PowerlinkError> {
    trace!(
        "Building SDO ASnd request for Node {} (TID {})",
        target_node_id.0, cmd.header.transaction_id
    );
    let Some(dest_mac) = scheduler::get_cn_mac_address(context, target_node_id) else {
        error!(
            "[MN] Cannot build SDO ASnd: MAC for Node {} not found.",
            target_node_id.0
        );
        return Err(PowerlinkError::InternalError("Missing CN MAC address"));
    };

    let sdo_payload = serialize_sdo_asnd_payload(seq_header, cmd)?;

    Ok(PowerlinkFrame::ASnd(ASndFrame::new(
        context.core.mac_address,
        dest_mac,
        target_node_id,
        NodeId(C_ADR_MN_DEF_NODE_ID),
        ServiceId::Sdo,
        sdo_payload,
    )))
}