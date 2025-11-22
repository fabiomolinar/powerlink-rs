// src/node/mn/tick.rs
//! Handles time-based events for the Managing Node (MN).

use super::cycle;
use super::events;
use super::payload;
use super::state::{CyclePhase, MnContext};
use crate::common::{NetTime, RelativeTime};
use crate::frame::PowerlinkFrame;
use crate::frame::control::SocFrame;
use crate::nmt::NmtStateMachine;
use crate::nmt::events::NmtEvent;
use crate::nmt::states::NmtState;
use crate::node::{NodeAction, serialize_frame_action};
use crate::od::constants;
use crate::sdo::SdoTransport;
use crate::sdo::server::SdoClientInfo;
use log::{error, info, trace, warn};

/// Handles periodic timer events for the node.
///
/// This function is the heartbeat of the MN's scheduling logic. It orchestrates:
/// 1. The start of the POWERLINK Cycle (SoC) or Reduced Cycle (SoA).
/// 2. SDO retransmissions and timeouts.
/// 3. NMT state timeouts (e.g., WaitNotActive).
/// 4. DLL timeouts (e.g., waiting for PRes).
pub(crate) fn handle_tick(context: &mut MnContext, current_time_us: u64) -> NodeAction {
    // --- 0. Check for Cycle Start ---
    let time_since_last_cycle = current_time_us.saturating_sub(context.current_cycle_start_time_us);
    let current_nmt_state = context.nmt_state_machine.current_state();

    // Spec 7.1.3.2.2.1: In NMT_MS_PRE_OPERATIONAL_1, start Reduced Cycle.
    // Spec 7.1.3.2.2.2+: In PreOp2/ReadyToOp/Op, start Isochronous Cycle.
    // Both are triggered here. The distinction is handled inside `cycle::start_cycle`
    // or `cycle::advance_cycle_phase`.
    if time_since_last_cycle >= context.cycle_time_us
        && current_nmt_state >= NmtState::NmtPreOperational1
        && context.current_phase == CyclePhase::Idle
    {
        trace!("[MN] Cycle time elapsed ({}us). Starting new cycle.", context.cycle_time_us);
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
        match payload::build_sdo_asnd_request(context, target_node_id, seq, cmd) {
            Ok(frame) => {
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
            // (Existing SDO Server logic preserved)
             match context
                .core
                .sdo_server
                .tick(current_time_us, &context.core.od)
            {
                Ok(Some(response_data)) => {
                    warn!("SDO Server tick generated abort frame.");
                    let build_result = match response_data.client_info {
                        SdoClientInfo::Asnd { .. } => {
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
                Ok(None) => {}
                Err(e) => error!("SDO server tick error: {:?}", e),
            }
        }
    }

    // --- 3. Check for NMT/Scheduler Deadlines ---
    
    let is_bootstrapping = current_nmt_state == NmtState::NmtNotActive 
                        && context.next_tick_us.is_none();

    let deadline_passed = context
        .next_tick_us
        .is_some_and(|deadline| current_time_us >= deadline);

    // Handle Timeouts
    if deadline_passed {
        trace!(
            "Tick deadline reached at {}us (Deadline was {:?})",
            current_time_us, context.next_tick_us
        );
        context.next_tick_us = None; // Consume deadline

        // Handle NmtNotActive Timeout
        if current_nmt_state == NmtState::NmtNotActive {
            info!("[MN] WaitNotActive timeout expired. Assuming MN role.");
            context.nmt_state_machine.process_event(NmtEvent::Timeout, &mut context.core.od);
            return NodeAction::NoAction;
        }
        
        // Handle PRes Timeout
        if let Some(event) = context.pending_timeout_event.take() {
            warn!("[MN] PRes timeout for Node {:?}.", context.current_polled_cn);
            events::handle_dll_event(
                context,
                event,
                // Dummy frame for context logging
                &PowerlinkFrame::Soc(SocFrame::new(
                    Default::default(),
                    Default::default(),
                    NetTime { seconds: 0, nanoseconds: 0 },
                    RelativeTime { seconds: 0, nanoseconds: 0 },
                )),
            );
            return cycle::advance_cycle_phase(context, current_time_us);
        }
    }

    // --- 4. Cycle Phase Progression (FIXED) ---
    // Even if no *timeout* occurred, if we are in an active phase (e.g., SoCSent, IsochronousDone),
    // we must check if logic dictates advancing the phase.
    // For example: SoCSent -> SoA (in PreOp1) happens immediately without a timer.
    
    if context.current_phase != CyclePhase::Idle && context.current_phase != CyclePhase::IsochronousPReq && context.current_phase != CyclePhase::AsynchronousSoA {
         // If we are PReq or SoA, we are waiting for a specific timeout or response.
         // If we are SoCSent, IsochronousDone, AwaitingMnAsyncSend, we proceed immediately.
         
         // Specifically check PreOp1 skip logic
         if current_nmt_state == NmtState::NmtPreOperational1 && context.current_phase == CyclePhase::SoCSent {
             return cycle::advance_cycle_phase(context, current_time_us);
         }

         // Allow cycle::tick to handle AwaitingMnAsyncSend and other logic
    }

    // Proceed to standard cycle tick logic (handling queues, etc.)
    // Only return NoAction if not bootstrapping AND no deadline passed AND phase is Idle.
    if !deadline_passed && !is_bootstrapping && context.current_phase == CyclePhase::Idle {
         return NodeAction::NoAction;
    }

    cycle::tick(context, current_time_us)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::error::{DllErrorManager, LoggingErrorHandler, MnErrorCounters};
    use crate::frame::ms_state_machine::DllMsStateMachine;
    use crate::frame::{DllMsEvent, PowerlinkFrame, deserialize_frame};
    use crate::nmt::mn_state_machine::MnNmtStateMachine;
    use crate::node::mn::state::CyclePhase;
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

    fn create_test_context<'a>() -> MnContext<'a> {
        let od = ObjectDictionary::new(None);
        let core = CoreNodeContext {
            od,
            mac_address: Default::default(),
            sdo_server: SdoServer::new(),
            sdo_client: SdoClient::new(),
            embedded_sdo_server: EmbeddedSdoServer::new(),
            embedded_sdo_client: EmbeddedSdoClient::new(),
        };

        MnContext {
            core,
            configuration_interface: None,
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
            cycle_time_us: 1000,
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
    fn test_handle_tick_starts_cycle() {
        let mut context = create_test_context();
        context.cycle_time_us = 1000;
        context.current_cycle_start_time_us = 1000;

        context
            .nmt_state_machine
            .set_state(NmtState::NmtOperational);
        context.current_phase = CyclePhase::Idle;

        let action1 = handle_tick(&mut context, 1900);
        assert!(matches!(action1, NodeAction::NoAction));

        let action2 = handle_tick(&mut context, 2000);

        if let NodeAction::SendFrame(bytes) = action2 {
            let frame = deserialize_frame(&bytes).expect("Failed to deserialize SoC");
            assert!(matches!(frame, PowerlinkFrame::Soc(_)));
        } else {
            panic!("Expected SendFrame(SoC)");
        }

        assert_eq!(context.current_cycle_start_time_us, 2000);
        assert_eq!(context.current_phase, CyclePhase::SoCSent);
    }

    #[test]
    fn test_handle_tick_pres_timeout() {
        let mut context = create_test_context();
        context
            .nmt_state_machine
            .set_state(NmtState::NmtOperational);

        // Fix: Set phase to IsochronousPReq so handle_tick doesn't try to start a new cycle
        // (which would preempt the timeout handling)
        context.current_phase = CyclePhase::IsochronousPReq;

        context.pending_timeout_event = Some(DllMsEvent::PresTimeout);
        context.current_polled_cn = Some(NodeId(5));
        context.next_tick_us = Some(1500);

        // Add an async request to force SoA transmission logic in advance_cycle_phase
        context
            .async_request_queue
            .push(crate::node::mn::state::AsyncRequest {
                node_id: NodeId(1),
                priority: 1,
            });

        let action = handle_tick(&mut context, 1500);

        assert!(
            context.pending_timeout_event.is_none()
                || context.pending_timeout_event == Some(DllMsEvent::AsndTimeout)
        );
        assert!(
            matches!(action, NodeAction::SendFrame(_)),
            "Should advance to next phase"
        );
    }
}