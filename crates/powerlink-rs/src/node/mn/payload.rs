// crates/powerlink-rs/src/node/mn/payload.rs
use super::scheduler;
use super::state::MnContext;
use crate::common::{NetTime, RelativeTime};
use crate::frame::basic::MacAddress;
use crate::frame::control::{SoAFlags, SocFlags};
use crate::frame::poll::PReqFlags; // Import directly
use crate::frame::{
    ASndFrame, PReqFrame, PowerlinkFrame, RequestedServiceId, ServiceId, SoAFrame, SocFrame,
};
use crate::nmt::events::NmtCommand;
use crate::nmt::NmtStateMachine; // Added for nmt_state()
use crate::od::{ObjectDictionary, ObjectValue};
use crate::pdo::{PDOVersion, PdoMappingEntry};
use crate::types::{C_ADR_BROADCAST_NODE_ID, C_ADR_MN_DEF_NODE_ID, EPLVersion, NodeId}; // Added C_ADR_BROADCAST_NODE_ID
use crate::PowerlinkError;
use alloc::vec;
use alloc::vec::Vec;
use log::{debug, error, info, trace, warn};

// Constants for OD access
const OD_IDX_TPDO_COMM_PARAM_BASE: u16 = 0x1800;
const OD_IDX_TPDO_MAPP_PARAM_BASE: u16 = 0x1A00;
const OD_SUBIDX_PDO_COMM_NODEID: u8 = 1;
const OD_SUBIDX_PDO_COMM_VERSION: u8 = 2;
const OD_IDX_MN_PREQ_PAYLOAD_LIMIT_LIST: u16 = 0x1F8B;
const OD_IDX_EPL_VERSION: u16 = 0x1F83; // Added for reading EPL version
// ERROR FIX: Added missing constants
const OD_IDX_MN_CYCLE_TIMING_REC: u16 = 0x1F98;
const OD_SUBIDX_ASYNC_SLOT_TIMEOUT: u8 = 2;

/// Builds a SoC frame.
pub(super) fn build_soc_frame(
    context: &MnContext,
    current_multiplex_cycle: u8,
    multiplex_cycle_len: u8,
) -> PowerlinkFrame {
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
    context: &MnContext,
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

    // Find TPDO mapping for this CN
    let mut pdo_channel = None;
    for i in 0..256 {
        let comm_param_index = OD_IDX_TPDO_COMM_PARAM_BASE + i as u16;
        if context
            .core.od
            .read_u8(comm_param_index, OD_SUBIDX_PDO_COMM_NODEID)
            == Some(target_node_id.0)
        {
            pdo_channel = Some(i as u8);
            break;
        }
    }

    let payload_result = pdo_channel.map_or(Ok((Vec::new(), PDOVersion(0))), |channel| {
        build_tpdo_payload(&context.core.od, channel)
    });

    match payload_result {
        Ok((payload, pdo_version)) => {
            let rd_flag =
                context.nmt_state_machine.current_state() == crate::nmt::states::NmtState::NmtOperational;
            // Get the last known EA flag for this node
            let ea_flag = context
                .node_info
                .get(&target_node_id)
                .map_or(false, |info| info.ea_flag);

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

/// Builds the payload for a TPDO (PReq) frame.
pub(super) fn build_tpdo_payload(
    od: &ObjectDictionary,
    channel_index: u8,
) -> Result<(Vec<u8>, PDOVersion), PowerlinkError> {
    let comm_param_index = OD_IDX_TPDO_COMM_PARAM_BASE + channel_index as u16;
    let mapping_index = OD_IDX_TPDO_MAPP_PARAM_BASE + channel_index as u16;
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
    let mut max_offset_len = 0;

    if let Some(ObjectValue::Unsigned8(num_entries)) = od.read(mapping_index, 0).as_deref() {
        for i in 1..=*num_entries {
            // ERROR FIX (E0716): Bind the Cow to a variable to extend its lifetime.
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
            let (Some(offset), Some(length)) = (entry.byte_offset(), entry.byte_length()) else {
                warn!(
                    "[MN] Bit-level TPDO mapping not supported for PReq 0x{:04X}/{}",
                    entry.index, entry.sub_index
                );
                continue;
            };
            let end_pos = offset + length;
            if end_pos > payload_limit {
                error!("TPDO mapping for PReq exceeds payload limit. Mapping invalid.");
                return Err(PowerlinkError::ValidationError(
                    "PDO mapping exceeds PReq payload limit",
                ));
            }
            max_offset_len = max_offset_len.max(end_pos);
            let Some(value_cow) = od.read(entry.index, entry.sub_index) else {
                warn!(
                    "[MN] TPDO mapping for PReq 0x{:04X}/{} failed: OD entry not found. Filling with zeros.",
                    entry.index, entry.sub_index
                );
                payload[offset..end_pos].fill(0);
                continue;
            };
            let serialized_data = value_cow.serialize();
            if serialized_data.len() != length {
                warn!(
                    "[MN] TPDO mapping for PReq 0x{:04X}/{} length mismatch. Mapped: {} bytes, Object: {} bytes. Truncating/Padding.",
                    entry.index,
                    entry.sub_index,
                    length,
                    serialized_data.len()
                );
                let copy_len = serialized_data.len().min(length);
                payload[offset..offset + copy_len].copy_from_slice(&serialized_data[..copy_len]);
                if length > copy_len {
                    payload[offset + copy_len..end_pos].fill(0);
                }
            } else {
                payload[offset..end_pos].copy_from_slice(&serialized_data);
            }
        }
    }
    payload.truncate(max_offset_len);
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
    let epl_version = EPLVersion(context.core.od.read_u8(OD_IDX_EPL_VERSION, 0).unwrap_or(0x15));

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

/// Builds and serializes an SoA(IdentRequest) frame.
pub(super) fn build_soa_ident_request(
    context: &MnContext,
    target_node_id: NodeId,
) -> PowerlinkFrame {
    debug!(
        "[MN] Building SoA(IdentRequest) for Node {}",
        target_node_id.0
    );
    let epl_version = EPLVersion(context.core.od.read_u8(OD_IDX_EPL_VERSION, 0).unwrap_or(0x15));
    let req_service = if target_node_id.0 == 0 {
        RequestedServiceId::NoService
    } else {
        RequestedServiceId::IdentRequest
    };
    PowerlinkFrame::SoA(SoAFrame::new(
        context.core.mac_address,
        context.nmt_state_machine.current_state(),
        SoAFlags::default(),
        req_service,
        target_node_id,
        epl_version,
    ))
}

/// Builds an ASnd(NMT Command) frame.
pub(super) fn build_nmt_command_frame(
    context: &MnContext,
    command: NmtCommand,
    target_node_id: NodeId,
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
    let nmt_payload = vec![command as u8, 0u8];
    PowerlinkFrame::ASnd(ASndFrame::new(
        context.core.mac_address,
        dest_mac,
        target_node_id,
        NodeId(C_ADR_MN_DEF_NODE_ID),
        ServiceId::NmtCommand,
        nmt_payload,
    ))
}
