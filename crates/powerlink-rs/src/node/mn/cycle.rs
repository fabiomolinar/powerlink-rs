// crates/powerlink-rs/src/node/mn/cycle.rs
use super::payload;
use super::scheduler;
use super::state::{CyclePhase, MnContext};
use crate::frame::DllMsEvent;
use crate::node::NodeAction;
use crate::types::C_ADR_MN_DEF_NODE_ID;
use log::debug;

// Constants for OD access used in this file.
const OD_IDX_MN_PRES_TIMEOUT_LIST: u16 = 0x1F92;
const OD_IDX_CYCLE_TIMING_REC: u16 = 0x1F98;
const OD_SUBIDX_ASYNC_SLOT_TIMEOUT: u8 = 2;

/// Advances the POWERLINK cycle to the next phase (e.g., next PReq or SoA).
pub(super) fn advance_cycle_phase(
    context: &mut MnContext,
    current_time_us: u64,
) -> NodeAction {
    // Check if there are more isochronous nodes to poll in the current cycle.
    if let Some(node_id) =
        scheduler::get_next_isochronous_node_to_poll(context, context.current_multiplex_cycle)
    {
        context.current_polled_cn = Some(node_id);
        context.current_phase = CyclePhase::IsochronousPReq;
        let timeout_ns = context
            .od
            .read_u32(OD_IDX_MN_PRES_TIMEOUT_LIST, node_id.0)
            .unwrap_or(25000) as u64;
        scheduler::schedule_timeout(
            context,
            current_time_us + (timeout_ns / 1000),
            DllMsEvent::PresTimeout,
        );
        let is_multiplexed = context.multiplex_assign.get(&node_id).copied().unwrap_or(0) > 0;
        let frame = payload::build_preq_frame(context, node_id, is_multiplexed);
        return scheduler::serialize_and_prepare_action(context, frame);
    }

    // No more isochronous nodes to poll, transition to the asynchronous phase.
    debug!(
        "[MN] Isochronous phase complete for cycle {}.",
        context.current_multiplex_cycle
    );
    context.current_polled_cn = None;
    context.current_phase = CyclePhase::IsochronousDone;

    let (req_service, target_node, set_er_flag) =
        scheduler::determine_next_async_action(context);

    if target_node.0 != C_ADR_MN_DEF_NODE_ID
        && req_service != crate::frame::RequestedServiceId::NoService
    {
        context.current_phase = CyclePhase::AsynchronousSoA;
        let timeout_ns = context
            .od
            .read_u32(OD_IDX_CYCLE_TIMING_REC, OD_SUBIDX_ASYNC_SLOT_TIMEOUT)
            .unwrap_or(100_000) as u64;
        scheduler::schedule_timeout(
            context,
            current_time_us + (timeout_ns / 1000),
            DllMsEvent::AsndTimeout,
        );
    } else if target_node.0 == C_ADR_MN_DEF_NODE_ID {
        context.current_phase = CyclePhase::AwaitingMnAsyncSend;
    } else {
        context.current_phase = CyclePhase::Idle;
    }

    let frame = payload::build_soa_frame(context, req_service, target_node, set_er_flag);
    scheduler::serialize_and_prepare_action(context, frame)
}
