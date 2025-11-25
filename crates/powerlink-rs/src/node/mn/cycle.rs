// src/node/mn/cycle.rs
use super::state::{CyclePhase, MnContext};
use crate::frame::{DllMsEvent, PowerlinkFrame};
use crate::nmt::NmtStateMachine;
use crate::nmt::events::{MnNmtCommandRequest, NmtStateCommand};
use crate::nmt::states::NmtState;
use crate::node::{NodeAction, serialize_frame_action};
use crate::od::constants;
use crate::types::{C_ADR_BROADCAST_NODE_ID, C_ADR_MN_DEF_NODE_ID, NodeId};
use crate::log::{pl_debug, pl_error, pl_info};

use super::events;
use super::payload;
use super::scheduler;
use crate::node::mn::state::NmtCommandData;

/// Advances the POWERLINK cycle to the next phase.
///
/// The Cycle State Machine (DLL_MS) dictates the sequence:
/// 1. SoC (Start of Cycle)
/// 2. Isochronous Phase (PReq -> PRes for each node)
/// 3. Asynchronous Phase (SoA -> ASnd)
///
/// Reference: EPSG DS 301, 4.2.4.6 MN Cycle State Machine
pub(super) fn advance_cycle_phase(context: &mut MnContext, current_time_us: u64) -> NodeAction {
    let current_nmt_state = context.nmt_state_machine.current_state();

    // Spec 4.2.4.2 Reduced POWERLINK Cycle:
    // "The Reduced POWERLINK Cycle shall consist of queued asynchronous phases only."
    // However, Figure 24 implies SoA is the start.
    // In NmtPreOperational1, we skip the Isochronous phase entirely.
    // Isochronous phase is only valid for PreOp2, ReadyToOp, and Operational.
    let isochronous_allowed = current_nmt_state >= NmtState::NmtPreOperational2;

    if isochronous_allowed {
        // --- Isochronous Phase (4.2.4.1.1) ---
        // Check if there are more isochronous nodes to poll in the current multiplex cycle.
        if let Some(node_id) =
            scheduler::get_next_isochronous_node_to_poll(context, context.current_multiplex_cycle)
        {
            context.current_polled_cn = Some(node_id);
            context.current_phase = CyclePhase::IsochronousPReq;
            
            // Set timeout for PRes (Spec 7.2.2.3.3 NMT_MNCNPResTimeout_AU32)
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

            // Check if node is multiplexed (for MS flag in PReq)
            let is_multiplexed = context.multiplex_assign.get(&node_id).copied().unwrap_or(0) > 0;
            let frame = payload::build_preq_frame(context, node_id, is_multiplexed);

            // Increment Isochronous Tx counter (Diag 0x1101)
            context.core.od.increment_counter(
                constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
                constants::SUBIDX_DIAG_NMT_COUNT_ISOCHR_TX,
            );

            return serialize_frame_action(frame, context).unwrap_or(
                NodeAction::NoAction,
            );
        }
    }

    // --- Transition to Asynchronous Phase ---
    // No more isochronous nodes to poll (or we are skipping them).
    if context.current_phase != CyclePhase::IsochronousDone {
        pl_debug!(*context, 
            "[MN] Isochronous phase complete for cycle {}. Phase: SoCSent -> SoA",
            context.current_multiplex_cycle
        );
    }
    
    context.current_polled_cn = None;
    context.current_phase = CyclePhase::IsochronousDone;

    // --- Check for NMT Info Service publishing (Spec 7.3.4) ---
    // Mapped via OD 0x1F9E (Publish Config)
    let current_mux_cycle_1_based = context.current_multiplex_cycle.wrapping_add(1);
    if let Some(&service_id) = context.publish_config.get(&current_mux_cycle_1_based) {
        pl_info!(*context, 
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

        // Call the payload builder
        let frame = payload::build_nmt_info_frame(context, service_id);
        return serialize_frame_action(frame, context).unwrap_or(NodeAction::NoAction);
    }
    // --- End of NMT Info Service logic ---

    // --- Asynchronous Phase (4.2.4.1.2) ---
    // Determine who gets the token (SoA).
    let (req_service, target_node, set_er_flag) = scheduler::determine_next_async_action(context);

    if target_node.0 != C_ADR_MN_DEF_NODE_ID
        && req_service != crate::frame::RequestedServiceId::NoService
    {
        // Granting token to a CN
        context.current_phase = CyclePhase::AsynchronousSoA;
        let timeout_ns = context
            .core
            .od
            .read_u32(
                constants::IDX_NMT_MN_CYCLE_TIMING_REC,
                constants::SUBIDX_NMT_MN_CYCLE_TIMING_ASYNC_SLOT_U32,
            )
            .unwrap_or(100_000) as u64;
            
        // Schedule timeout waiting for ASnd from CN
        scheduler::schedule_timeout(
            context,
            current_time_us + (timeout_ns / 1000),
            DllMsEvent::AsndTimeout,
        );
    } else if target_node.0 == C_ADR_MN_DEF_NODE_ID {
        // MN Grants token to self (AwaitingMnAsyncSend)
        context.current_phase = CyclePhase::AwaitingMnAsyncSend;
    } else {
        // No service (Idle)
        context.current_phase = CyclePhase::Idle;
    }

    // Increment Async Tx counter (for SoA)
    context.core.od.increment_counter(
        constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
        constants::SUBIDX_DIAG_NMT_COUNT_ASYNC_TX,
    );

    // Increment StatusRequest counter if applicable
    if req_service == crate::frame::RequestedServiceId::StatusRequest {
        context.core.od.increment_counter(
            constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
            constants::SUBIDX_DIAG_NMT_COUNT_STATUS_REQ,
        );
    }

    let frame = payload::build_soa_frame(context, req_service, target_node, set_er_flag);
    serialize_frame_action(frame, context).unwrap_or(
        NodeAction::NoAction,
    )
}

/// Starts a new cycle by sending a SoC.
/// This is the implementation of the SocTrig event.
pub(super) fn start_cycle(context: &mut MnContext, current_time_us: u64) -> NodeAction {
    // 1. Update RelativeTime Accumulator
    // EPSG 301 4.6.1.1.2: "The RelativeTime shall be incremented by the value of NMT_CycleLen_U32."
    // It starts at 0 on boot and accumulates every cycle.
    // NMT_CycleLen is stored in context.cycle_time_us (read from OD 0x1006).
    context.relative_time_accumulator.add_micros(context.cycle_time_us);

    // 2. Update cycle timing and multiplexing
    context.current_cycle_start_time_us = current_time_us;
    if context.multiplex_cycle_len > 0 {
        context.current_multiplex_cycle =
            (context.current_multiplex_cycle + 1) % context.multiplex_cycle_len;
    }
    context.next_isoch_node_idx = 0; // Reset for this cycle's polling

    // 3. Build the SoC frame
    let soc_frame = payload::build_soc_frame(
        context,
        context.current_multiplex_cycle,
        context.multiplex_cycle_len,
    );

    // 4. Notify the DLL state machine of the SocTrig
    // We pass the frame we're *about* to send as the context
    events::handle_dll_event(context, DllMsEvent::SocTrig, &soc_frame);

    // 5. Update internal state
    // The DLL state machine (handle_dll_event) should have moved us to a new state.
    // Based on spec, it's likely WaitPres (DLL_MT1) or WaitAsnd (DLL_MT6)
    // We set our phase to SoCSent so the *next* tick in main.rs triggers advance_cycle_phase
    context.current_phase = CyclePhase::SoCSent;

    // 6. Return the frame to be sent
    // Increment Isochronous Cycle counter (this is also done by CN)
    context.core.od.increment_counter(
        constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
        constants::SUBIDX_DIAG_NMT_COUNT_ISOCHR_CYC,
    );

    match serialize_frame_action(soc_frame, context) {
        Ok(action) => action,
        Err(e) => {
            pl_error!(*context, "[MN] Failed to serialize SoC frame: {:?}", e);
            NodeAction::NoAction
        }
    }
}

/// The MN's main scheduler tick for non-cycle-start events.
pub(super) fn tick(context: &mut MnContext, current_time_us: u64) -> NodeAction {
    let current_nmt_state = context.nmt_state_machine.current_state();

    // --- 1. Handle one-time actions ---
    // Example: Sending NMTStartNode when entering Operational (Spec 7.4.1.6)
    if current_nmt_state == NmtState::NmtOperational && !context.initial_operational_actions_done {
        context.initial_operational_actions_done = true;
        if (context.nmt_state_machine.startup_flags & (1 << 1)) != 0 {
            pl_info!(*context, "[MN] Sending NMTStartNode (Broadcast).");
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
                NodeAction::NoAction,
            );
        } else if let Some(&node_id) = context.mandatory_nodes.first() {
            pl_info!(*context, "[MN] Queuing NMTStartNode (Unicast).");
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
            // MN has invited itself via SoA. Check what to send.
            // Priority: NMT Commands > SDO Client > Generic Queue
            // we consume the phase here, but we might set it to Idle or keep processing
            context.current_phase = CyclePhase::Idle; 

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

            // SDO Client
            if let Some((target_node_id, seq, cmd)) = context
                .sdo_client_manager
                .get_pending_request(current_time_us, &context.core.od)
            {
                match payload::build_sdo_asnd_request(context, target_node_id, seq, cmd) {
                    Ok(frame) => {
                        // *** INCREMENT SDO TX COUNTER (ASnd Request) ***
                        context.core.od.increment_counter(
                            constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
                            constants::SUBIDX_DIAG_NMT_COUNT_SDO_TX,
                        );
                        return serialize_frame_action(frame, context)
                            .unwrap_or(NodeAction::NoAction);
                    }
                    Err(e) => pl_error!(*context, "Failed to build SDO client request frame: {:?}", e),
                }
            }

            // Generic Async Queue (e.g. NMT Info)
            if let Some(frame) = context.mn_async_send_queue.pop() {
                // *** INCREMENT ASYNC TX COUNTER (Generic) ***
                context.core.od.increment_counter(
                    constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
                    constants::SUBIDX_DIAG_NMT_COUNT_ASYNC_TX,
                );
                return serialize_frame_action(frame, context).unwrap_or(NodeAction::NoAction);
            }

            // If we got here, we invited ourselves but had nothing to send.
            pl_debug!(*context, "[MN] Awaited async send, but no frames were queued.");
            return NodeAction::NoAction;
        }
        CyclePhase::SoCSent => {
            // Immediately advance to PReq (if allowed) or SoA
            return advance_cycle_phase(context, current_time_us);
        }
        _ => {}
    }

    // --- 3. Handle time-based actions (Bootstrapping) ---
    // Spec 7.1.3.2.1: If no SoC/SoA seen for MNWaitNotAct, transition to PreOp1.
    if current_nmt_state == NmtState::NmtNotActive && context.next_tick_us.is_none() {
        let timeout_us = context.nmt_state_machine.wait_not_active_timeout as u64;
        context.next_tick_us = Some(current_time_us + timeout_us);
        return NodeAction::NoAction;
    }

    NodeAction::NoAction
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::{NetTime, RelativeTime};
    use crate::frame::error::{DllErrorManager, LoggingErrorHandler, MnErrorCounters};
    use crate::frame::ms_state_machine::DllMsStateMachine;
    use crate::hal::TimeProvider;
    use crate::nmt::mn_state_machine::MnNmtStateMachine;
    use crate::node::mn::state::{AsyncRequest, CnInfo, CnState};
    use crate::node::{CoreNodeContext, NodeAction};
    use crate::od::ObjectDictionary;
    use crate::sdo::client_manager::SdoClientManager;
    use crate::sdo::transport::AsndTransport;
    #[cfg(feature = "sdo-udp")]
    use crate::sdo::transport::UdpTransport;
    use crate::sdo::{EmbeddedSdoClient, EmbeddedSdoServer, SdoClient, SdoServer};
    use crate::types::{C_ADR_MN_DEF_NODE_ID, NodeId};
    use alloc::collections::{BTreeMap, BinaryHeap};
    use alloc::vec::Vec;

    // --- Mock TimeProvider ---
    struct MockTimeProvider;
    impl TimeProvider for MockTimeProvider {
        fn now_monotonic_us(&self) -> u64 { 0 }
        fn now_net_time(&self) -> NetTime { NetTime::default() }
    }
    // --- End Mock ---

    fn create_test_context<'a>(time_provider: &'a dyn TimeProvider) -> MnContext<'a> {
        let od = ObjectDictionary::new(None);
        let core = CoreNodeContext {
            od,
            node_id: NodeId(C_ADR_MN_DEF_NODE_ID),
            mac_address: Default::default(),
            sdo_server: SdoServer::new(),
            sdo_client: SdoClient::new(),
            embedded_sdo_server: EmbeddedSdoServer::new(),
            embedded_sdo_client: EmbeddedSdoClient::new(),
        };

        MnContext {
            core,
            configuration_interface: None,
            time_provider,
            relative_time_accumulator: RelativeTime::default(),
            nmt_state_machine: MnNmtStateMachine::new(
                NodeId(C_ADR_MN_DEF_NODE_ID),
                Default::default(),
                0,
                0,
            ),
            dll_state_machine: DllMsStateMachine::default(),
            dll_error_manager: DllErrorManager::new(MnErrorCounters::new(), LoggingErrorHandler),
            asnd_transport: AsndTransport,
            #[cfg(feature = "sdo-udp")]
            udp_transport: UdpTransport,
            cycle_time_us: 10000,
            multiplex_cycle_len: 0,
            multiplex_assign: BTreeMap::new(),
            publish_config: BTreeMap::new(),
            current_multiplex_cycle: 0,
            node_info: BTreeMap::new(),
            mandatory_nodes: Vec::new(),
            isochronous_nodes: Vec::new(),
            async_only_nodes: Vec::new(),
            arp_cache: BTreeMap::new(),
            next_isoch_node_idx: 0,
            current_phase: CyclePhase::Idle,
            current_polled_cn: None,
            async_request_queue: BinaryHeap::new(),
            pending_er_requests: Vec::new(),
            pending_status_requests: Vec::new(),
            pending_nmt_commands: Vec::new(),
            mn_async_send_queue: Vec::new(),
            sdo_client_manager: SdoClientManager::new(),
            last_ident_poll_node_id: NodeId(0),
            last_status_poll_node_id: NodeId(0),
            next_tick_us: None,
            pending_timeout_event: None,
            current_cycle_start_time_us: 0,
            initial_operational_actions_done: false,
        }
    }

    #[test]
    fn test_start_cycle_sets_phase_and_sends_soc() {
        let mock_time = MockTimeProvider;
        let mut context = create_test_context(&mock_time);
        let current_time = 1000;

        let action = start_cycle(&mut context, current_time);

        assert_eq!(context.current_phase, CyclePhase::SoCSent);
        assert_eq!(context.current_cycle_start_time_us, current_time);
        // Should send SoC
        if let NodeAction::SendFrame(_) = action {
            // OK
        } else {
            panic!("Expected SendFrame(SoC)");
        }
    }

    #[test]
    fn test_advance_phase_soc_to_isochronous_no_nodes() {
        let mock_time = MockTimeProvider;
        let mut context = create_test_context(&mock_time);
        
        // 1. Set state to Operational so `isochronous_allowed` is true.
        context.nmt_state_machine.set_state(crate::nmt::states::NmtState::NmtOperational);
        context.current_phase = CyclePhase::SoCSent;

        // 2. Add an AsyncRequest. 
        // Without this, the scheduler returns "NoService" and the phase goes to Idle.
        // We need a request to force the transition to `AsynchronousSoA`.
        context.async_request_queue.push(AsyncRequest {
            node_id: NodeId(10),
            priority: 1,
        });

        // No isochronous nodes configured in context.isochronous_nodes by default.

        let action = advance_cycle_phase(&mut context, 2000);

        // Should skip directly to AsynchronousSoA because isochronous nodes are empty
        // AND we have pending async work.
        assert_eq!(context.current_phase, CyclePhase::AsynchronousSoA);
        
        if let NodeAction::SendFrame(_) = action {
            // OK (SoA)
        } else {
            panic!("Expected SendFrame(SoA)");
        }
    }

    #[test]
    fn test_handle_asynchronous_phase_priority_queue() {
        let mock_time = MockTimeProvider;
        let mut context = create_test_context(&mock_time);
        context.current_phase = CyclePhase::IsochronousDone;

        // Add a high priority request
        context.async_request_queue.push(AsyncRequest {
            node_id: NodeId(5),
            priority: 7,
        });
        // Add a low priority request
        context.async_request_queue.push(AsyncRequest {
            node_id: NodeId(2),
            priority: 1,
        });

        let action = advance_cycle_phase(&mut context, 3000);

        assert_eq!(context.current_phase, CyclePhase::AsynchronousSoA);
        // We can't easily inspect the frame content here without deserializing,
        // but it should be an SoA with ReqServiceId for Node 5.
        if let NodeAction::SendFrame(_) = action {
            // OK
        } else {
            panic!("Expected SendFrame(SoA)");
        }

        // Queue should still have the lower priority one?
        // Actually, pop() removes it. We only serve one per cycle.
        assert_eq!(context.async_request_queue.len(), 1);
        assert_eq!(context.async_request_queue.peek().unwrap().node_id, NodeId(2));
    }
}