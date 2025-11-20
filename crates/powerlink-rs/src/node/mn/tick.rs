// crates/powerlink-rs/src/node/mn/tick.rs
//! Handles time-based events for the Managing Node (MN).
//! Includes Cycle Start, SDO Timeouts, and General NMT/Scheduler Ticks.

use super::cycle;
use super::events;
use super::state::{CyclePhase, MnContext};
use crate::common::{NetTime, RelativeTime};
use crate::frame::control::SocFrame;
use crate::frame::{DllMsEvent, PowerlinkFrame};
use crate::nmt::NmtStateMachine;
use crate::nmt::states::NmtState;
use crate::node::{NodeAction, serialize_frame_action};
use crate::od::constants;
use crate::sdo::server::SdoClientInfo;
#[cfg(feature = "sdo-udp")]
use crate::sdo::transport::UdpTransport;
use crate::sdo::transport::{AsndTransport, SdoTransport};
use log::{error, trace, warn};

/// Handles periodic timer events for the node.
pub(crate) fn handle_tick(context: &mut MnContext, current_time_us: u64) -> NodeAction {
    // --- 0. Check for Cycle Start ---
    let time_since_last_cycle =
        current_time_us.saturating_sub(context.current_cycle_start_time_us);
    let current_nmt_state = context.nmt_state_machine.current_state();

    if time_since_last_cycle >= context.cycle_time_us
        && current_nmt_state >= NmtState::NmtPreOperational2
        && context.current_phase == CyclePhase::Idle
    {
        trace!("[MN] Cycle time elapsed. Starting new cycle.");
        return cycle::start_cycle(context, current_time_us);
    }

    // --- 1. Check for SDO Client Timeouts ---
    if let Some((target_node_id, seq, cmd)) = context
        .sdo_client_manager
        .tick(current_time_us, &context.core.od)
    {
        warn!(
            "SDO Client tick generated frame (timeout/abort) for Node {}.",
            target_node_id.0
        );
        // An SDO client timeout/abort needs to send a frame.
        match cycle::build_sdo_asnd_request(context, target_node_id, seq, cmd) {
            Ok(frame) => {
                // *** INCREMENT SDO TX COUNTER (ASnd Client Abort) ***
                context.core.od.increment_counter(
                    constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
                    constants::SUBIDX_DIAG_NMT_COUNT_SDO_TX,
                );
                return serialize_frame_action(frame, context).unwrap_or(NodeAction::NoAction);
            }
            Err(e) => error!("Failed to build SDO client tick frame: {:?}", e),
        }
    }

    // --- 2. Check for SDO Server Timeouts ---
    if let Some(deadline) = context.core.sdo_server.next_action_time() {
        if current_time_us >= deadline {
            match context
                .core
                .sdo_server
                .tick(current_time_us, &context.core.od)
            {
                Ok(Some(response_data)) => {
                    // SDO server timed out, needs to send an Abort.
                    warn!("SDO Server tick generated abort frame.");
                    let build_result = match response_data.client_info {
                        SdoClientInfo::Asnd { .. } => {
                            // *** INCREMENT SDO TX COUNTER (ASnd Server Abort) ***
                            context.core.od.increment_counter(
                                constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
                                constants::SUBIDX_DIAG_NMT_COUNT_SDO_TX,
                            );
                            context
                                .asnd_transport
                                .build_response(response_data, context)
                        }
                        #[cfg(feature = "sdo-udp")]
                        SdoClientInfo::Udp { .. } => {
                            // *** INCREMENT SDO TX COUNTER (UDP Server Abort) ***
                            context.core.od.increment_counter(
                                constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
                                constants::SUBIDX_DIAG_NMT_COUNT_SDO_TX,
                            );
                            context.udp_transport.build_response(response_data, context)
                        }
                    };
                    match build_result {
                        Ok(action) => return action,
                        Err(e) => {
                            error!("Failed to build SDO/ASnd abort response: {:?}", e);
                        }
                    }
                }
                Ok(None) => {} // Tick processed, no action
                Err(e) => error!("SDO server tick error: {:?}", e),
            }
        }
    }

    // --- 3. Check for NMT/Scheduler Timeouts ---
    let deadline_passed = context
        .next_tick_us
        .is_some_and(|deadline| current_time_us >= deadline);

    if !deadline_passed {
        return NodeAction::NoAction;
    }

    // A deadline has passed
    trace!(
        "Tick deadline reached at {}us (Deadline was {:?})",
        current_time_us, context.next_tick_us
    );
    context.next_tick_us = None; // Consume deadline

    // --- Handle PRes Timeout ---
    if let Some(event) = context.pending_timeout_event.take() {
        // This is a PRes timeout
        warn!("[MN] PRes timeout for Node {:?}.", context.current_polled_cn);
        events::handle_dll_event(
            context,
            event,
            &PowerlinkFrame::Soc(SocFrame::new(
                Default::default(),
                Default::default(),
                NetTime {
                    seconds: 0,
                    nanoseconds: 0,
                },
                RelativeTime {
                    seconds: 0,
                    nanoseconds: 0,
                },
            )),
        );
        // A PRes timeout means we must advance the cycle.
        return cycle::advance_cycle_phase(context, current_time_us);
    } else {
        // This is a general NMT tick (e.g., for async SDO polls)
        cycle::tick(context, current_time_us)
    }
}