// crates/powerlink-rs/src/node/mn/events.rs
use super::main::ManagingNode;
use super::scheduler;
use super::state::{AsyncRequest, CnState, CyclePhase};
use crate::Node;
use crate::frame::{
    ASndFrame, DllMsEvent, PResFrame, PowerlinkFrame, ServiceId,
    error::{DllError, NmtAction},
};
use crate::nmt::NmtStateMachine;
use crate::nmt::{events::NmtEvent, states::NmtState};
use crate::node::PdoHandler;
use crate::types::NodeId;
use log::{debug, info, trace, warn};

/// Internal function to process a deserialized `PowerlinkFrame`.
/// The MN primarily *consumes* PRes and ASnd frames.
pub(super) fn process_frame(node: &mut ManagingNode, frame: PowerlinkFrame, current_time_us: u64) {
    // 1. Update NMT state machine based on the frame type.
    if let Some(event) = frame.nmt_event() {
        if node.nmt_state() != NmtState::NmtNotActive {
            node.nmt_state_machine.process_event(event, &mut node.od);
        }
    }

    // 2. Pass event to DLL state machine and handle errors.
    handle_dll_event(node, frame.dll_mn_event(), &frame);

    // 3. Handle specific frames
    match frame {
        PowerlinkFrame::PRes(pres_frame) => {
            // Update CN state based on reported NMT state
            update_cn_state(node, pres_frame.source, pres_frame.nmt_state);

            // Check if this PRes corresponds to the node we polled
            if node.current_phase == CyclePhase::IsochronousPReq
                && node.current_polled_cn == Some(pres_frame.source)
            {
                trace!(
                    "[MN] Received expected PRes from Node {}",
                    pres_frame.source.0
                );
                // Cancel pending PRes timeout
                node.pending_timeout_event = None;
                // Handle PDO consumption from PRes frames
                node.consume_pdo_payload(
                    pres_frame.source,
                    &pres_frame.payload,
                    pres_frame.pdo_version,
                    pres_frame.flags.rd,
                );
                // Handle async requests flagged in PRes
                handle_pres_flags(node, &pres_frame);
                // PRes received, advance to the next action in the cycle.
                let action = node.advance_cycle_phase(current_time_us);
                // TODO: The action needs to be returned to the main loop to be sent.
                // This is a current limitation of the refactoring.
            } else {
                warn!(
                    "[MN] Received unexpected PRes from Node {}.",
                    pres_frame.source.0
                );
                handle_pres_flags(node, &pres_frame);
            }
        }
        PowerlinkFrame::ASnd(asnd_frame) => {
            if node.current_phase == CyclePhase::AsynchronousSoA {
                trace!(
                    "[MN] Received ASnd from Node {} during Async phase.",
                    asnd_frame.source.0
                );
                node.pending_timeout_event = None;
                handle_asnd_frame(node, &asnd_frame);
                node.current_phase = CyclePhase::Idle;
            } else {
                handle_asnd_frame(node, &asnd_frame);
            }
        }
        _ => {
            // MN ignores SoC, PReq, and SoA frames it sent itself.
        }
    }
}

/// Passes an event to the DLL state machine and processes any resulting errors.
pub(super) fn handle_dll_event(
    node: &mut ManagingNode,
    event: DllMsEvent,
    frame_context: &PowerlinkFrame,
) {
    let reporting_node_id = match frame_context {
        PowerlinkFrame::PRes(f) => f.source,
        PowerlinkFrame::ASnd(f) => f.source,
        _ => node.current_polled_cn.unwrap_or(NodeId(0)),
    };
    let isochr_nodes_remaining =
        scheduler::has_more_isochronous_nodes(node, node.current_multiplex_cycle);
    let isochr = isochr_nodes_remaining || node.current_phase == CyclePhase::IsochronousPReq;

    if let Some(errors) = node.dll_state_machine.process_event(
        event,
        node.nmt_state(),
        matches!(event, DllMsEvent::Pres | DllMsEvent::Asnd),
        !node.async_request_queue.is_empty(),
        !node.mn_async_send_queue.is_empty(),
        isochr,
        false,
        reporting_node_id,
    ) {
        for error in errors {
            warn!("[MN] DLL state machine reported error: {:?}", error);
            let error_with_node = match error {
                DllError::LossOfPres { .. } => DllError::LossOfPres {
                    node_id: reporting_node_id,
                },
                _ => error,
            };
            let (nmt_action, _) = node.dll_error_manager.handle_error(error_with_node);
            match nmt_action {
                NmtAction::ResetNode(node_id) => {
                    warn!(
                        "[MN] DLL Error threshold met for Node {}. Requesting Node Reset.",
                        node_id.0
                    );
                    if let Some(state) = node.node_states.get_mut(&node_id) {
                        *state = CnState::Missing;
                    }
                    node.pending_nmt_commands
                        .push((crate::nmt::events::NmtCommand::ResetNode, node_id));
                }
                NmtAction::ResetCommunication => {
                    warn!("[MN] DLL Error threshold met. Requesting Communication Reset.");
                    node.nmt_state_machine
                        .process_event(NmtEvent::Error, &mut node.od);
                    node.current_phase = CyclePhase::Idle;
                }
                NmtAction::None => {}
            }
        }
    }
}

/// Handles incoming ASnd frames, such as IdentResponse or StatusResponse.
fn handle_asnd_frame(node: &mut ManagingNode, frame: &ASndFrame) {
    match frame.service_id {
        ServiceId::IdentResponse => {
            let node_id = frame.source;
            if let Some(state) = node.node_states.get_mut(&node_id) {
                if *state == CnState::Unknown || *state == CnState::Missing {
                    *state = CnState::Identified;
                    info!("[MN] Node {} identified.", node_id.0);
                    scheduler::check_bootup_state(node);
                }
            } else {
                warn!(
                    "[MN] Received IdentResponse from unconfigured Node {}.",
                    node_id.0
                );
            }
        }
        ServiceId::StatusResponse => {
            trace!(
                "[MN] Received StatusResponse from CN {}. Processing not yet fully implemented.",
                frame.source.0
            );
        }
        _ => {
            trace!(
                "[MN] Received unhandled ASnd with ServiceID {:?}.",
                frame.service_id
            );
        }
    }
}

/// Checks the flags in a received PRes frame and queues async requests.
fn handle_pres_flags(node: &mut ManagingNode, pres: &PResFrame) {
    if pres.flags.rs.get() > 0 {
        debug!("[MN] Node {} requesting async transmission.", pres.source.0);
        node.async_request_queue.push(AsyncRequest {
            node_id: pres.source,
            priority: pres.flags.pr as u8,
        });
    }
}

/// Updates the MN's internal state tracker for a CN based on its reported NMT state.
fn update_cn_state(node: &mut ManagingNode, node_id: NodeId, reported_state: NmtState) {
    if let Some(current_state_ref) = node.node_states.get_mut(&node_id) {
        let new_state = match reported_state {
            NmtState::NmtPreOperational1 => CnState::Identified,
            NmtState::NmtPreOperational2 | NmtState::NmtReadyToOperate => CnState::PreOperational,
            NmtState::NmtOperational => CnState::Operational,
            NmtState::NmtCsStopped => CnState::Stopped,
            _ => *current_state_ref,
        };
        if *current_state_ref != new_state {
            info!(
                "[MN] Node {} state changed: {:?} -> {:?}",
                node_id.0, *current_state_ref, new_state
            );
            *current_state_ref = new_state;
            scheduler::check_bootup_state(node);
        }
    }
}
