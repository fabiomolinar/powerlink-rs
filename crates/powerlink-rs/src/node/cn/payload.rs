// crates/powerlink-rs/src/node/cn/payload.rs
use crate::frame::basic::MacAddress;
use crate::frame::control::{IdentResponsePayload, StaticErrorBitField, StatusResponsePayload};
use crate::frame::error::ErrorEntry;
use crate::frame::poll::{PResFlags, RSFlag};
use crate::frame::{ASndFrame, PResFrame, PowerlinkFrame, ServiceId};
use crate::nmt::NmtStateMachine;
use crate::nmt::events::CnNmtRequest; // Import CnNmtRequest
use crate::nmt::states::NmtState;
use crate::od::constants;
use crate::pdo::PDOVersion;
use crate::sdo::SdoClient;
use crate::types::C_ADR_MN_DEF_NODE_ID;
use crate::{od::ObjectDictionary, types::NodeId};
use alloc::collections::VecDeque;
use alloc::vec;
use alloc::vec::Vec;
use log::{debug, error};

// Import the context directly for the new TPDO build method
use super::state::CnContext;

/// Builds an `ASnd` frame for the `IdentResponse` service.
pub(super) fn build_ident_response(
    mac_address: MacAddress,
    node_id: NodeId,
    od: &ObjectDictionary,
    soa: &crate::frame::SoAFrame,
    sdo_client: &SdoClient,
    pending_nmt_requests: &[(CnNmtRequest, NodeId)], // Updated type
) -> PowerlinkFrame {
    debug!("Building IdentResponse for SoA from node {}", soa.source.0);

    // --- New logic using IdentResponsePayload struct ---
    let mut payload_struct = IdentResponsePayload::new(od);

    // Set PR/RS flags based on pending requests
    let (rs_count, pr_flag) = if !pending_nmt_requests.is_empty() {
        // NMT requests (Commands or Services) always have the highest priority
        (
            pending_nmt_requests.len().min(7) as u8,
            crate::frame::PRFlag::PrioNmtRequest,
        )
    } else {
        sdo_client.pending_request_count_and_priority()
    };
    payload_struct.pr = pr_flag;
    payload_struct.rs = RSFlag::new(rs_count);

    // Serialize the payload
    let mut payload_buf = vec![0u8; 158]; // IDENT_RESPONSE_PAYLOAD_SIZE
    let payload_len = match payload_struct.serialize(&mut payload_buf) {
        Ok(len) => len,
        Err(e) => {
            error!("Failed to serialize IdentResponsePayload: {:?}", e);
            158 // Failsafe, send zeroed payload
        }
    };
    payload_buf.truncate(payload_len);
    // --- End of new logic ---

    let asnd = ASndFrame::new(
        mac_address,
        soa.eth_header.source_mac,    // Send back to the MN's MAC
        NodeId(C_ADR_MN_DEF_NODE_ID), // Destination Node ID is MN
        node_id,
        ServiceId::IdentResponse,
        payload_buf, // Use the serialized struct
    );
    PowerlinkFrame::ASnd(asnd)
}

/// Builds an `ASnd` frame for the `StatusResponse` service.
pub(super) fn build_status_response(
    mac_address: MacAddress,
    node_id: NodeId,
    od: &mut ObjectDictionary,
    en_flag: bool,
    ec_flag: bool,
    emergency_queue: &mut VecDeque<ErrorEntry>,
    soa: &crate::frame::SoAFrame,
    sdo_client: &SdoClient,
    pending_nmt_requests: &[(CnNmtRequest, NodeId)], // Updated type
) -> PowerlinkFrame {
    debug!("Building StatusResponse for SoA from node {}", soa.source.0);

    // --- New logic using StatusResponsePayload struct ---
    let nmt_state = od
        .read_u8(constants::IDX_NMT_CURR_NMT_STATE_U8, 0)
        .and_then(|val| NmtState::try_from(val).ok())
        .unwrap_or(NmtState::NmtNotActive);

    let static_errors = StaticErrorBitField::new(od);

    // Set PR/RS flags
    let (rs_count, pr_flag) = if !pending_nmt_requests.is_empty() {
        // NMT requests (Commands or Services) always have the highest priority
        (
            pending_nmt_requests.len().min(7) as u8,
            crate::frame::PRFlag::PrioNmtRequest,
        )
    } else {
        sdo_client.pending_request_count_and_priority()
    };

    // Determine max payload size from AsyncMTU
    let mtu = od
        .read_u16(
            constants::IDX_NMT_CYCLE_TIMING_REC,
            constants::SUBIDX_NMT_CYCLE_TIMING_ASYNC_MTU_U16,
        )
        .unwrap_or(300) as usize;
    let max_payload_size = mtu.saturating_sub(4); // 4 bytes for ASnd header

    // Calculate max entries that fit (Header(14) + Terminator(20))
    let max_entries = (max_payload_size.saturating_sub(14 + 20)) / 20;

    // Drain the emergency queue up to the max that will fit
    let entries: Vec<ErrorEntry> = emergency_queue
        .drain(..max_entries.min(emergency_queue.len()))
        .collect();

    // Increment emergency write counter if we are sending entries
    if !entries.is_empty() {
        od.increment_counter(
            constants::IDX_DIAG_ERR_STATISTICS_REC,
            constants::SUBIDX_DIAG_ERR_STATS_EMCY_WRITE,
        );
    }

    let mut payload_struct = StatusResponsePayload::new(
        en_flag,
        ec_flag,
        pr_flag,
        RSFlag::new(rs_count),
        nmt_state,
        static_errors,
        entries,
    );

    // Serialize the payload
    let mut payload_buf = vec![0u8; max_payload_size];
    let payload_len = match payload_struct.serialize(&mut payload_buf) {
        Ok(len) => len,
        Err(e) => {
            error!("Failed to serialize StatusResponsePayload: {:?}", e);
            // Failsafe: send just the header + terminator
            payload_buf.truncate(14 + 20);
            payload_buf.fill(0);
            // Re-serialize with empty entries
            payload_struct.error_entries = Vec::new();
            payload_struct
                .serialize(&mut payload_buf)
                .unwrap_or(14 + 20)
        }
    };
    payload_buf.truncate(payload_len);
    // --- End of new logic ---

    let asnd = ASndFrame::new(
        mac_address,
        soa.eth_header.source_mac,    // Send back to MN's MAC
        NodeId(C_ADR_MN_DEF_NODE_ID), // Destination is MN
        node_id,
        ServiceId::StatusResponse,
        payload_buf, // Use the serialized struct
    );
    PowerlinkFrame::ASnd(asnd)
}

/// Builds an `ASnd` frame for the `NMTRequest` service.
pub(super) fn build_nmt_request(
    mac_address: MacAddress,
    node_id: NodeId,
    command_id: u8, // Updated type to u8
    target: NodeId,
    soa: &crate::frame::SoAFrame,
) -> PowerlinkFrame {
    debug!(
        "Building NMTRequest(CommandID={:#04x}, Target={}) for SoA from node {}",
        command_id, target.0, soa.source.0
    );
    // Payload format from Spec Table 144
    let payload = vec![
        command_id, // NMTRequestedCommandID
        target.0,   // NMTRequestedCommandTarget
                    // NMTRequestedCommandData is omitted for plain commands/services
    ];

    let asnd = ASndFrame::new(
        mac_address,
        soa.eth_header.source_mac,
        NodeId(C_ADR_MN_DEF_NODE_ID),
        node_id,
        ServiceId::NmtRequest,
        payload,
    );
    PowerlinkFrame::ASnd(asnd)
}

/// Builds a `PRes` frame in response to being polled by a `PReq`.
pub(super) fn build_pres_response(context: &mut CnContext, en_flag: bool) -> PowerlinkFrame {
    let node_id = context.nmt_state_machine.node_id();
    let nmt_state = context.nmt_state_machine.current_state();
    let mac_address = context.core.mac_address;

    debug!("Building PRes in response to PReq for node {}", node_id.0);

    // Call the new inherent method on CnContext, which is now mutable
    let (payload, pdo_version, payload_is_valid) = match context.build_tpdo_payload() {
        Ok((payload, version)) => (payload, version, true),
        Err(e) => {
            error!(
                "Failed to build TPDO payload for PRes: {:?}. Sending empty PRes with RD=0.",
                e
            );
            // Must still send a PRes of the correct configured size, even if empty
            let payload_limit = context
                .core
                .od
                .read_u16(
                    constants::IDX_NMT_CYCLE_TIMING_REC,
                    constants::SUBIDX_NMT_CYCLE_TIMING_PRES_ACT_PAYLOAD_U16,
                )
                .unwrap_or(36) as usize;
            (vec![0u8; payload_limit], PDOVersion(0), false)
        }
    };

    // The RD flag is set only if the NMT state is Operational AND the PDO payload was built successfully.
    let rd_flag = (nmt_state == NmtState::NmtOperational) && payload_is_valid;

    // Check for pending SDO/NMT requests to set RS and PR flags.
    // NMT requests (PR=7) have higher priority than generic SDO requests (PR=3).
    let (rs_count, pr_flag) = if !context.pending_nmt_requests.is_empty() {
        (
            context.pending_nmt_requests.len().min(7) as u8,
            crate::frame::PRFlag::PrioNmtRequest,
        )
    } else {
        context.core.sdo_client.pending_request_count_and_priority()
    };

    let flags = PResFlags {
        rd: rd_flag,
        en: en_flag,
        rs: RSFlag::new(rs_count),
        pr: pr_flag,
        ..Default::default()
    };

    let pres = PResFrame::new(mac_address, node_id, nmt_state, flags, pdo_version, payload);
    PowerlinkFrame::PRes(pres)
}
