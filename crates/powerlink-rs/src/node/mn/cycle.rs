// crates/powerlink-rs/src/node/mn/cycle.rs
use super::payload;
use super::scheduler;
use super::state::{CyclePhase, MnContext};
use crate::frame::{DllMsEvent, RequestedServiceId}; // Added RequestedServiceId
use crate::node::{NodeAction, serialize_frame_action};
use crate::od::constants; // Import the new constants module
use crate::types::C_ADR_MN_DEF_NODE_ID;
use log::debug;

/// Advances the POWERLINK cycle to the next phase (e.g., next PReq or SoA).
pub(super) fn advance_cycle_phase(context: &mut MnContext, current_time_us: u64) -> NodeAction {
    // Check if there are more isochronous nodes to poll in the current cycle.
    if let Some(node_id) =
        scheduler::get_next_isochronous_node_to_poll(context, context.current_multiplex_cycle)
    {
        context.current_polled_cn = Some(node_id);
        context.current_phase = CyclePhase::IsochronousPReq;
        let timeout_ns = context
            .core
            .od
            .read_u32(constants::IDX_NMT_MN_CN_PRES_TIMEOUT_AU32, node_id.0)
            .unwrap_or(25000) as u64;
        scheduler::schedule_timeout(
            context,
            current_time_us + (timeout_ns / 1000),
            DllMsEvent::PresTimeout,
        );
        let is_multiplexed = context.multiplex_assign.get(&node_id).copied().unwrap_or(0) > 0;
        let frame = payload::build_preq_frame(context, node_id, is_multiplexed);

        // Increment Isochronous Tx counter
        context.core.od.increment_counter(
            constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
            constants::SUBIDX_DIAG_NMT_COUNT_ISOCHR_TX,
        );

        return serialize_frame_action(frame, context).unwrap_or(
            // TODO: handle error
            NodeAction::NoAction,
        );
    }

    // No more isochronous nodes to poll, transition to the asynchronous phase.
    debug!(
        "[MN] Isochronous phase complete for cycle {}.",
        context.current_multiplex_cycle
    );
    context.current_polled_cn = None;
    context.current_phase = CyclePhase::IsochronousDone;

    let (req_service, target_node, set_er_flag) = scheduler::determine_next_async_action(context);

    if target_node.0 != C_ADR_MN_DEF_NODE_ID
        && req_service != crate::frame::RequestedServiceId::NoService
    {
        context.current_phase = CyclePhase::AsynchronousSoA;
        let timeout_ns = context
            .core
            .od
            .read_u32(
                constants::IDX_NMT_MN_CYCLE_TIMING_REC,
                constants::SUBIDX_NMT_MN_CYCLE_TIMING_ASYNC_SLOT_U32,
            )
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

    // Increment StatusRequest counter if applicable
    if req_service == RequestedServiceId::StatusRequest {
        context.core.od.increment_counter(
            constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
            constants::SUBIDX_DIAG_NMT_COUNT_STATUS_REQ,
        );
    }

    let frame = payload::build_soa_frame(context, req_service, target_node, set_er_flag);
    serialize_frame_action(frame, context).unwrap_or(
        // TODO: handle error properly
        NodeAction::NoAction,
    )
}