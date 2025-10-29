use super::main::ManagingNode;
use super::scheduler;
use super::state::{AsyncRequest, CnInfo, CnState, CyclePhase};
use crate::Node;
use crate::frame::{
    ASndFrame, DllMsEvent, PResFrame, PowerlinkFrame, ServiceId,
    error::{DllError, ErrorEntry, ErrorEntryMode, NmtAction, StaticErrorBitField},
};
use crate::nmt::NmtStateMachine;
use crate::nmt::{events::NmtEvent, states::NmtState};
use crate::node::PdoHandler;
use crate::types::NodeId;
use log::{debug, error, info, trace, warn};

/// Internal function to process a deserialized `PowerlinkFrame`.
/// The MN primarily *consumes* PRes and ASnd frames.
/// This function is now called by `ManagingNode::process_powerlink_frame`
/// and does not handle SDO frames directly anymore.
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
                // Handle async and error signaling flags in PRes
                handle_pres_frame(node, &pres_frame);
                // PRes received, advance to the next action in the cycle.
                let _action = node.advance_cycle_phase(current_time_us);
                // TODO: The action needs to be returned to the main loop to be sent.
                // This is a current limitation of the refactoring.
            } else {
                warn!(
                    "[MN] Received unexpected PRes from Node {}.",
                    pres_frame.source.0
                );
                handle_pres_frame(node, &pres_frame);
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
                    if let Some(info) = node.node_info.get_mut(&node_id) {
                        info.state = CnState::Missing;
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
            if let Some(info) = node.node_info.get_mut(&node_id) {
                if info.state == CnState::Unknown || info.state == CnState::Missing {
                    info.state = CnState::Identified;
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
            let node_id = frame.source;
            trace!("[MN] Received StatusResponse from CN {}.", frame.source.0);
            if let Some(info) = node.node_info.get_mut(&node_id) {
                // The handshake is complete. Update the MN's EA flag to match the CN's EN flag.
                // This new EA value will be sent in the next PReq.
                info.ea_flag = info.en_flag;
                info!(
                    "[MN] StatusResponse from Node {} processed. Updated EA flag to {}.",
                    node_id.0, info.ea_flag
                );
                // Process the actual error information in the StatusResponse payload.
                process_status_response_payload(frame.source, &frame.payload);
            }
        }
        _ => {
            trace!(
                "[MN] Received unhandled ASnd with ServiceID {:?}.",
                frame.service_id
            );
        }
    }
}

/// Parses and logs the contents of a StatusResponse payload.
/// (Reference: EPSG DS 301, Section 7.3.3.3.1)
fn process_status_response_payload(source_node: NodeId, payload: &[u8]) {
    // The StatusResponse payload starts at offset 6 within the ASnd service slot.
    if payload.len() < 6 {
        warn!(
            "[MN] Received StatusResponse from Node {} with invalid short payload ({} bytes).",
            source_node.0,
            payload.len()
        );
        return;
    }
    let status_payload = &payload[6..];

    // 1. Parse Static Error Bit Field (first 8 bytes)
    if status_payload.len() < 8 {
        warn!(
            "[MN] StatusResponse payload from Node {} is too short for Static Error Bit Field.",
            source_node.0
        );
        return;
    }
    match StaticErrorBitField::deserialize(status_payload) {
        Ok(field) => {
            info!(
                "[MN] StatusResponse from Node {}: ErrorRegister = {:#04x}, SpecificErrors = {:02X?}",
                source_node.0, field.error_register, field.specific_errors
            );
        }
        Err(e) => {
            error!(
                "[MN] Failed to parse StaticErrorBitField from Node {}: {:?}",
                source_node.0, e
            );
            return;
        }
    }

    // 2. Parse list of Error/Event History Entries (in 20-byte chunks)
    let mut offset = 8;
    while offset + 20 <= status_payload.len() {
        let entry_slice = &status_payload[offset..offset + 20];
        match ErrorEntry::deserialize(entry_slice) {
            Ok(entry) => {
                // The list is terminated by an entry with Mode = Terminator
                if entry.entry_type.mode == ErrorEntryMode::Terminator {
                    trace!(
                        "[MN] End of StatusResponse error entries for Node {}.",
                        source_node.0
                    );
                    break;
                }
                warn!(
                    "[MN] StatusResponse from Node {}: Received Error/Event Entry: {:?}",
                    source_node.0, entry
                );
                // In a real application, this entry would be processed or stored.
                offset += 20;
            }
            Err(e) => {
                error!(
                    "[MN] Failed to parse ErrorEntry from Node {}: {:?}",
                    source_node.0, e
                );
                break; // Stop parsing on error
            }
        }
    }
}

/// Checks the flags in a received PRes frame for async requests and error signals.
fn handle_pres_frame(node: &mut ManagingNode, pres: &PResFrame) {
    // 1. Handle async requests flagged by RS.
    if pres.flags.rs.get() > 0 {
        debug!("[MN] Node {} requesting async transmission.", pres.source.0);
        node.async_request_queue.push(AsyncRequest {
            node_id: pres.source,
            priority: pres.flags.pr as u8,
        });
    }

    // 2. Handle error signaling with EN/EA flags.
    if let Some(info) = node.node_info.get_mut(&pres.source) {
        // Store the received EN flag from the CN
        info.en_flag = pres.flags.en;

        // Spec 6.5.6: If the MN detects that the last sent EA bit is different to
        // the last received EN bit, it shall send a StatusRequest frame to the CN.
        if info.en_flag != info.ea_flag {
            info!(
                "[MN] Detected EN/EA mismatch for Node {}. (EN={}, EA={}). Queuing StatusRequest.",
                pres.source.0, info.en_flag, info.ea_flag
            );
            // Add the node to a queue to be polled with a StatusRequest.
            // Avoid adding duplicates if a request is already pending.
            if !node.pending_status_requests.contains(&pres.source) {
                node.pending_status_requests.push(pres.source);
            }
        }
    }
}

/// Updates the MN's internal state tracker for a CN based on its reported NMT state.
fn update_cn_state(node: &mut ManagingNode, node_id: NodeId, reported_state: NmtState) {
    if let Some(current_info) = node.node_info.get_mut(&node_id) {
        let new_state = match reported_state {
            NmtState::NmtPreOperational1 => CnState::Identified,
            NmtState::NmtPreOperational2 | NmtState::NmtReadyToOperate => CnState::PreOperational,
            NmtState::NmtOperational => CnState::Operational,
            NmtState::NmtCsStopped => CnState::Stopped,
            _ => current_info.state,
        };
        if current_info.state != new_state {
            info!(
                "[MN] Node {} state changed: {:?} -> {:?}",
                node_id.0, current_info.state, new_state
            );
            current_info.state = new_state;
            scheduler::check_bootup_state(node);
        }
    }
}