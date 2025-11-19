// crates/powerlink-rs/src/node/mn/cycle.rs
use super::state::{CnInfo, CyclePhase, MnContext};
use crate::PowerlinkError;
use crate::frame::{DllMsEvent, PowerlinkFrame, RequestedServiceId};
use crate::nmt::NmtStateMachine;
use crate::nmt::events::{MnNmtCommandRequest, NmtStateCommand}; // Updated imports
use crate::nmt::states::NmtState;
use crate::node::{NodeAction, serialize_frame_action};
use crate::od::{Object, ObjectDictionary, ObjectValue, constants};
use crate::types::{C_ADR_BROADCAST_NODE_ID, C_ADR_MN_DEF_NODE_ID, NodeId};
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use log::{debug, error, info, trace};

use super::events; // Import events
use super::payload;
use super::scheduler;
use crate::frame::ASndFrame; // Import ASndFrame
use crate::frame::ServiceId;
use crate::node::mn::state::NmtCommandData; // Import NmtCommandData
use crate::sdo::asnd::serialize_sdo_asnd_payload; // Import SDO ASnd serializer
use crate::sdo::command::SdoCommand; // Import SdoCommand
use crate::sdo::sequence::SequenceLayerHeader; // Import SequenceLayerHeader // Import ServiceId

/// Parses the MN's OD configuration to build its internal node lists.
/// This function was missing from the provided context.
pub(super) fn parse_mn_node_lists(
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

    // --- Check for NMT Info Service publishing ---
    // The multiplexed cycle number in the OD (0x1F9E) is 1-based.
    let current_mux_cycle_1_based = context.current_multiplex_cycle.wrapping_add(1);
    if let Some(&service_id) = context.publish_config.get(&current_mux_cycle_1_based) {
        info!(
            "[MN] Publishing NMT Info Service {:?} for Mux Cycle {}.",
            service_id, current_mux_cycle_1_based
        );
        // NMTPublish replaces SoA. The cycle ends, and we wait for the next SocTrig.
        context.current_phase = CyclePhase::Idle;

        // Increment Async Tx counter
        context.core.od.increment_counter(
            constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
            constants::SUBIDX_DIAG_NMT_COUNT_ASYNC_TX,
        );

        // Call the new payload builder
        let frame = payload::build_nmt_info_frame(context, service_id);
        return serialize_frame_action(frame, context).unwrap_or(NodeAction::NoAction);
    }
    // --- End of NMT Info Service logic ---

    // --- Original logic: Send SoA ---
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

    // Increment Async Tx counter (for SoA)
    context.core.od.increment_counter(
        constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
        constants::SUBIDX_DIAG_NMT_COUNT_ASYNC_TX,
    );

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

/// Starts a new isochronous cycle by sending a SoC.
/// This is the implementation of the SocTrig event.
pub(super) fn start_cycle(context: &mut MnContext, current_time_us: u64) -> NodeAction {
    // 1. Update cycle timing and multiplexing
    context.current_cycle_start_time_us = current_time_us;
    if context.multiplex_cycle_len > 0 {
        context.current_multiplex_cycle =
            (context.current_multiplex_cycle + 1) % context.multiplex_cycle_len;
    }
    context.next_isoch_node_idx = 0; // Reset for this cycle's polling

    // 2. Build the SoC frame
    let soc_frame = payload::build_soc_frame(
        context,
        context.current_multiplex_cycle,
        context.multiplex_cycle_len,
    );

    // 3. Notify the DLL state machine of the SocTrig
    // We pass the frame we're *about* to send as the context
    events::handle_dll_event(context, DllMsEvent::SocTrig, &soc_frame);

    // 4. Update internal state
    // The DLL state machine (handle_dll_event) should have moved us to a new state.
    // Based on spec, it's likely WaitPres (DLL_MT1) or WaitAsnd (DLL_MT6)
    // We set our phase to SoCSent so the *next* tick in main.rs triggers advance_cycle_phase
    context.current_phase = CyclePhase::SoCSent;

    // 5. Return the frame to be sent
    // Increment Isochronous Cycle counter (this is also done by CN)
    context.core.od.increment_counter(
        constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
        constants::SUBIDX_DIAG_NMT_COUNT_ISOCHR_CYC,
    );

    match serialize_frame_action(soc_frame, context) {
        Ok(action) => action,
        Err(e) => {
            error!("[MN] Failed to serialize SoC frame: {:?}", e);
            NodeAction::NoAction
        }
    }
}

/// The MN's main scheduler tick for non-cycle-start events.
/// Renamed from run_scheduler.
pub(super) fn tick(context: &mut MnContext, current_time_us: u64) -> NodeAction {
    let current_nmt_state = context.nmt_state_machine.current_state();

    // --- 1. Handle one-time actions ---
    if current_nmt_state == NmtState::NmtOperational && !context.initial_operational_actions_done {
        context.initial_operational_actions_done = true;
        if (context.nmt_state_machine.startup_flags & (1 << 1)) != 0 {
            info!("[MN] Sending NMTStartNode (Broadcast).");
            // *** INCREMENT ASYNC TX COUNTER ***
            context.core.od.increment_counter(
                constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
                constants::SUBIDX_DIAG_NMT_COUNT_ASYNC_TX,
            );
            return serialize_frame_action(
                payload::build_nmt_command_frame(
                    context,
                    MnNmtCommandRequest::State(NmtStateCommand::StartNode),
                    NodeId(C_ADR_BROADCAST_NODE_ID),
                    NmtCommandData::None,
                ),
                context,
            )
            .unwrap_or(
                // TODO: Handle error properly
                NodeAction::NoAction,
            );
        } else if let Some(&node_id) = context.mandatory_nodes.first() {
            info!("[MN] Queuing NMTStartNode (Unicast).");
            context.pending_nmt_commands.push((
                MnNmtCommandRequest::State(NmtStateCommand::StartNode),
                node_id,
                NmtCommandData::None,
            ));
        }
    } else if current_nmt_state < NmtState::NmtOperational {
        context.initial_operational_actions_done = false;
    }

    // --- 2. Handle immediate, non-time-based follow-up actions ---
    match context.current_phase {
        CyclePhase::AwaitingMnAsyncSend => {
            // MN has invited itself. Check what to send.
            // Priority: NMT Commands > SDO Client > Generic Queue
            // This logic was missing.
            context.current_phase = CyclePhase::Idle; // Consume the phase

            if let Some((command_req, target_node_id, command_data)) =
                context.pending_nmt_commands.pop()
            {
                // *** INCREMENT ASYNC TX COUNTER (NMT Command) ***
                context.core.od.increment_counter(
                    constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
                    constants::SUBIDX_DIAG_NMT_COUNT_ASYNC_TX,
                );
                return serialize_frame_action(
                    payload::build_nmt_command_frame(
                        context,
                        command_req,
                        target_node_id,
                        command_data,
                    ),
                    context,
                )
                .unwrap_or(NodeAction::NoAction);
            }

            if let Some((target_node_id, seq, cmd)) = context
                .sdo_client_manager
                .get_pending_request(current_time_us, &context.core.od)
            {
                match build_sdo_asnd_request(context, target_node_id, seq, cmd) {
                    Ok(frame) => {
                        // *** INCREMENT SDO TX COUNTER (ASnd Request) ***
                        context.core.od.increment_counter(
                            constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
                            constants::SUBIDX_DIAG_NMT_COUNT_SDO_TX,
                        );
                        return serialize_frame_action(frame, context)
                            .unwrap_or(NodeAction::NoAction);
                    }
                    Err(e) => error!("Failed to build SDO client request frame: {:?}", e),
                }
            }

            if let Some(frame) = context.mn_async_send_queue.pop() {
                // *** INCREMENT ASYNC TX COUNTER (Generic) ***
                context.core.od.increment_counter(
                    constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
                    constants::SUBIDX_DIAG_NMT_COUNT_ASYNC_TX,
                );
                return serialize_frame_action(frame, context).unwrap_or(NodeAction::NoAction);
            }

            // If we got here, we invited ourselves but had nothing to send.
            debug!("[MN] Awaited async send, but no frames were queued.");
            return NodeAction::NoAction;
        }
        CyclePhase::SoCSent => {
            return advance_cycle_phase(context, current_time_us);
        }
        _ => {}
    }

    // --- 3. Handle time-based actions ---
    if current_nmt_state == NmtState::NmtNotActive && context.next_tick_us.is_none() {
        let timeout_us = context.nmt_state_machine.wait_not_active_timeout as u64;
        context.next_tick_us = Some(current_time_us + timeout_us);
        return NodeAction::NoAction;
    }

    NodeAction::NoAction
}

/// Builds an ASnd(SDO Request) frame for the SdoClientManager.
/// This function was missing and is required by main.rs.
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
