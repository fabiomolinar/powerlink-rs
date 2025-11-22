// src/node/cn/tick.rs
//! Handles time-based events for the Controlled Node (CN).
//! Includes SDO timeouts, Heartbeat monitoring, and NMT state timeouts.

use super::state::CnContext;
use crate::common::NetTime;
use crate::frame::error::{EntryType, ErrorEntry, ErrorEntryMode};
use crate::frame::{DllCsEvent, DllError, NmtAction};
use crate::nmt::events::NmtEvent;
use crate::nmt::state_machine::NmtStateMachine;
use crate::nmt::states::NmtState;
use crate::node::NodeAction;
use crate::od::constants;
use crate::od::error_history; 
use crate::sdo::server::SdoClientInfo;
use crate::sdo::transport::SdoTransport;
use alloc::vec::Vec;
use crate::log::{my_debug, my_error, my_trace, my_warn};

/// Processes a timeout or other periodic check.
pub(crate) fn process_tick(context: &mut CnContext, current_time_us: u64) -> NodeAction {
    // --- SDO Server Tick (handles timeouts/retransmissions) ---
    // Spec 6.3.2.3.2.5: Broken Connection (Timeout)
    match context
        .core
        .sdo_server
        .tick(current_time_us, &context.core.od)
    {
        Ok(Some(response_data)) => {
            // SDO server generated a response (e.g., abort). Build the action.
            let build_result = match response_data.client_info {
                SdoClientInfo::Asnd { .. } => {
                    // *** INCREMENT SDO TX COUNTER (ASnd Abort) ***
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
                    // *** INCREMENT SDO TX COUNTER (UDP Abort) ***
                    context.core.od.increment_counter(
                        constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
                        constants::SUBIDX_DIAG_NMT_COUNT_SDO_TX,
                    );
                    context.udp_transport.build_response(response_data, context)
                }
            };
            if let Ok(action) = build_result {
                return action;
            }
        }
        Err(e) => my_error!("[CN] SDO Server tick error: {:?}", e),
        _ => {} 
    }

    let current_nmt_state = context.nmt_state_machine.current_state();

    // --- Heartbeat Consumer Check ---
    // Spec 7.3.5: NMT Guard Services
    // Spec 7.2.1.5.4: NMT_ConsumerHeartbeatTime_AU32
    if current_nmt_state >= NmtState::NmtPreOperational2 {
        let mut timed_out_nodes = Vec::new();
        for (node_id, (timeout_us, last_seen_us)) in &mut context.heartbeat_consumers {
            if *last_seen_us == 0 {
                // First tick in a valid state, initialize last_seen_us to now
                *last_seen_us = current_time_us;
            } else if *timeout_us > 0 && (current_time_us - *last_seen_us > *timeout_us) {
                my_warn!(
                    "[CN] Heartbeat timeout for Node {}! Last seen {}us ago (timeout is {}us).",
                    node_id.0,
                    current_time_us - *last_seen_us,
                    *timeout_us
                );
                timed_out_nodes.push(*node_id);
                *last_seen_us = current_time_us;
            }
        }

        // Handle errors outside the mutable borrow
        for node_id in timed_out_nodes {
            // Log error as HeartbeatTimeout (Custom DLL Error)
            let (nmt_action, signaled) = context
                .dll_error_manager
                .handle_error(DllError::HeartbeatTimeout { node_id });

            // Spec 6.5: Error Signaling (Update Error Register, Queue Emergency)
            if signaled {
                context.error_status_changed = true;
                let current_err_reg = context
                    .core
                    .od
                    .read_u8(constants::IDX_NMT_ERROR_REGISTER_U8, 0)
                    .unwrap_or(0);
                let new_err_reg = current_err_reg | 0b1; 
                if current_err_reg != new_err_reg {
                     context.core.od.increment_counter(
                        constants::IDX_DIAG_ERR_STATISTICS_REC,
                        constants::SUBIDX_DIAG_ERR_STATS_STATIC_ERR_CHG,
                    );
                    let _ = context.core.od.write_internal(
                        constants::IDX_NMT_ERROR_REGISTER_U8,
                        0,
                        crate::od::ObjectValue::Unsigned8(new_err_reg),
                        false,
                    );
                }
            }
            
            // Check if error triggers NMT state transition (e.g., to PreOp1)
            if nmt_action != NmtAction::None {
                context
                    .nmt_state_machine
                    .process_event(NmtEvent::Error, &mut context.core.od);
                context.soc_timeout_check_active = false;
                return NodeAction::NoAction;
            }
        }
    }

    // --- Handle NmtNotActive Timeout Setup (Bootstrapping) ---
    // Spec 7.2.3.1.1: NMT_CNBasicEthernetTimeout_U32 (0x1F99)
    // Transition from NotActive to BasicEthernet if no traffic seen.
    if current_nmt_state == NmtState::NmtNotActive && context.next_tick_us.is_none() {
        let timeout_us = context.nmt_state_machine.basic_ethernet_timeout as u64;
        if timeout_us > 0 {
            let deadline = current_time_us + timeout_us;
            context.next_tick_us = Some(deadline);
            my_debug!(
                "[CN] NmtNotActive: Starting BasicEthernet timeout check ({}us). Deadline: {}us",
                timeout_us, deadline
            );
        } else {
            my_debug!("[CN] NmtNotActive: BasicEthernet timeout is 0, check disabled.");
        }
        return NodeAction::NoAction;
    }

    // Check if a deadline is set and if it has passed
    let deadline_passed = context
        .next_tick_us
        .is_some_and(|deadline| current_time_us >= deadline);

    if !deadline_passed {
        return NodeAction::NoAction;
    }

    // --- A deadline has passed ---
    my_trace!(
        "Tick deadline reached at {}us (Deadline was {:?})",
        current_time_us, context.next_tick_us
    );
    context.next_tick_us = None; // Consume the deadline

    // --- Handle Specific Timeouts ---
    
    // 1. NmtNotActive -> BasicEthernet
    if current_nmt_state == NmtState::NmtNotActive {
        let timeout_us = context.nmt_state_machine.basic_ethernet_timeout as u64;
        if timeout_us > 0 {
            my_warn!("[CN] BasicEthernet timeout expired. Transitioning state.");
            context
                .nmt_state_machine
                .process_event(NmtEvent::Timeout, &mut context.core.od);
            context.soc_timeout_check_active = false;
        }
        return NodeAction::NoAction; 
    }
    
    // 2. SoC Timeout Check (Spec 4.7.7.3.1 Loss of SoC)
    if context.soc_timeout_check_active {
        my_warn!(
            "SoC timeout detected at {}us! Last SoC was at {}us.",
            current_time_us, context.last_soc_reception_time_us
        );
        // Trigger DLL Event
        if let Some(errors) = context
            .dll_state_machine
            .process_event(DllCsEvent::SocTimeout, current_nmt_state)
        {
            for error in errors {
                context.core.od.increment_counter(
                    constants::IDX_DIAG_ERR_STATISTICS_REC,
                    constants::SUBIDX_DIAG_ERR_STATS_HIST_WRITE,
                );
                let (nmt_action, signaled) = context.dll_error_manager.handle_error(error);
                
                if signaled {
                    context.error_status_changed = true;
                    // Update Error Register (0x1001)
                    let current_err_reg = context
                        .core
                        .od
                        .read_u8(constants::IDX_NMT_ERROR_REGISTER_U8, 0)
                        .unwrap_or(0);
                    let new_err_reg = current_err_reg | 0b1; // Set Generic Error
                    if current_err_reg != new_err_reg {
                        context.core.od.increment_counter(
                            constants::IDX_DIAG_ERR_STATISTICS_REC,
                            constants::SUBIDX_DIAG_ERR_STATS_STATIC_ERR_CHG,
                        );
                    }
                    let _ = context.core.od.write_internal(
                        constants::IDX_NMT_ERROR_REGISTER_U8,
                        0,
                        crate::od::ObjectValue::Unsigned8(new_err_reg),
                        false,
                    );

                    // Queue Emergency
                    let error_entry = ErrorEntry {
                        entry_type: EntryType {
                            is_status_entry: false,
                            send_to_queue: true,
                            mode: ErrorEntryMode::EventOccurred,
                            profile: 0x002,
                        },
                        error_code: error.to_error_code(),
                        timestamp: NetTime {
                            seconds: (current_time_us / 1_000_000) as u32,
                            nanoseconds: ((current_time_us % 1_000_000) * 1000) as u32,
                        },
                        additional_information: match error {
                            DllError::LossOfPres { node_id }
                            | DllError::LatePres { node_id }
                            | DllError::LossOfStatusRes { node_id } => node_id.0 as u64,
                            _ => 0,
                        },
                    };
                    if context.emergency_queue.len() < context.emergency_queue.capacity() {
                        context.emergency_queue.push_back(error_entry.clone());
                        error_history::write_error_to_history(&mut context.core.od, &error_entry);
                        
                        my_trace!("[CN] New error queued: {:?}", error_entry);
                        // Increment emergency write counter
                        context.core.od.increment_counter(
                            constants::IDX_DIAG_ERR_STATISTICS_REC,
                            constants::SUBIDX_DIAG_ERR_STATS_EMCY_WRITE,
                        );
                    } else {
                        my_warn!(
                            "[CN] Emergency queue full, dropping error: {:?}",
                            error_entry
                        );
                        // Increment emergency overflow counter
                        context.core.od.increment_counter(
                            constants::IDX_DIAG_ERR_STATISTICS_REC,
                            constants::SUBIDX_DIAG_ERR_STATS_EMCY_OVERFLOW,
                        );
                    }
                }
                
                // Handle NMT State Transition (ResetCommunication -> PreOp1)
                if nmt_action != NmtAction::None {
                    context
                        .nmt_state_machine
                        .process_event(NmtEvent::Error, &mut context.core.od);
                    context.soc_timeout_check_active = false;
                    return NodeAction::NoAction;
                }
            }
        }
        // Reschedule next check if still active (Spec 4.7.7.3.1)
        if context.soc_timeout_check_active {
            let cycle_time_opt = context
                .core
                .od
                .read_u32(constants::IDX_NMT_CYCLE_LEN_U32, 0)
                .map(|v| v as u64);
            // Tolerance in 0x1C14
            let tolerance_opt = context
                .core
                .od
                .read_u32(constants::IDX_DLL_CN_LOSS_OF_SOC_TOL_U32, 0)
                .map(|v| v as u64);

            if let (Some(cycle_time_us), Some(tolerance_ns)) = (cycle_time_opt, tolerance_opt) {
                if cycle_time_us > 0 {
                    let cycles_missed = ((current_time_us - context.last_soc_reception_time_us)
                        / cycle_time_us)
                        + 1;
                    let next_expected_soc_time =
                        context.last_soc_reception_time_us + cycles_missed * cycle_time_us;
                    let next_deadline = next_expected_soc_time + (tolerance_ns / 1000);
                    context.next_tick_us = Some(next_deadline);
                    my_trace!(
                        "SoC timeout occurred, scheduling next check at {}us",
                        next_deadline
                    );
                } else {
                    context.soc_timeout_check_active = false;
                }
            } else {
                context.soc_timeout_check_active = false;
            }
        }
    } else {
        my_trace!(
            "Tick deadline reached, but no specific timeout active (State: {:?}).",
            current_nmt_state
        );
    }

    NodeAction::NoAction // Default return if no frame needs sending
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::DllCsEvent;
    use crate::frame::cs_state_machine::DllCsStateMachine;
    use crate::frame::error::{CnErrorCounters, DllErrorManager, LoggingErrorHandler};
    use crate::nmt::cn_state_machine::CnNmtStateMachine;
    use crate::node::CoreNodeContext;
    use crate::node::cn::state::CnContext;
    use crate::od::{Object, ObjectDictionary, ObjectEntry, ObjectValue};
    use crate::sdo::transport::AsndTransport;
    #[cfg(feature = "sdo-udp")]
    use crate::sdo::transport::UdpTransport;
    use crate::sdo::{EmbeddedSdoClient, EmbeddedSdoServer, SdoClient, SdoServer};
    use crate::types::NodeId;
    use alloc::collections::{BTreeMap, VecDeque};
    use alloc::vec;
    use alloc::vec::Vec;

    fn create_context<'a>() -> CnContext<'a> {
        let mut od = ObjectDictionary::new(None);
        od.insert(
            constants::IDX_NMT_CYCLE_LEN_U32,
            ObjectEntry {
                object: Object::Variable(ObjectValue::Unsigned32(0)),
                ..Default::default()
            },
        );
        od.insert(
            constants::IDX_DLL_CN_LOSS_OF_SOC_TOL_U32,
            ObjectEntry {
                object: Object::Variable(ObjectValue::Unsigned32(0)),
                ..Default::default()
            },
        );
        od.insert(
            constants::IDX_NMT_ERROR_REGISTER_U8,
            ObjectEntry {
                object: Object::Variable(ObjectValue::Unsigned8(0)),
                ..Default::default()
            },
        );
        od.insert(
            constants::IDX_DIAG_ERR_STATISTICS_REC,
            ObjectEntry {
                object: Object::Record(vec![ObjectValue::Unsigned32(0); 16]),
                ..Default::default()
            },
        );

        let core = CoreNodeContext {
            od,
            mac_address: Default::default(),
            sdo_server: SdoServer::new(),
            sdo_client: SdoClient::new(),
            embedded_sdo_server: EmbeddedSdoServer::new(),
            embedded_sdo_client: EmbeddedSdoClient::new(),
        };

        CnContext {
            core,
            nmt_state_machine: CnNmtStateMachine::new(NodeId(1), Default::default(), 0),
            dll_state_machine: DllCsStateMachine::default(),
            dll_error_manager: DllErrorManager::new(CnErrorCounters::new(), LoggingErrorHandler),
            asnd_transport: AsndTransport,
            #[cfg(feature = "sdo-udp")]
            udp_transport: UdpTransport,
            pending_nmt_requests: Vec::new(),
            emergency_queue: VecDeque::new(),
            heartbeat_consumers: BTreeMap::new(),
            last_soc_reception_time_us: 0,
            soc_timeout_check_active: false,
            next_tick_us: None,
            en_flag: false,
            ec_flag: false,
            error_status_changed: false,
        }
    }

    #[test]
    fn test_heartbeat_timeout() {
        let mut context = create_context();
        context.heartbeat_consumers.insert(NodeId(240), (1000, 0));
        context
            .nmt_state_machine
            .set_state(NmtState::NmtPreOperational2);

        process_tick(&mut context, 100);

        for i in 0..20 {
            process_tick(&mut context, 1200 + (i * 1000));
            if context.error_status_changed {
                break;
            }
        }

        assert!(
            context.error_status_changed,
            "Heartbeat timeout failed to signal error"
        );
    }

    #[test]
    fn test_soc_timeout_logic() {
        let mut context = create_context();
        context
            .core
            .od
            .write(
                constants::IDX_NMT_CYCLE_LEN_U32,
                0,
                ObjectValue::Unsigned32(1000),
            )
            .unwrap();
        context
            .core
            .od
            .write(
                constants::IDX_DLL_CN_LOSS_OF_SOC_TOL_U32,
                0,
                ObjectValue::Unsigned32(100000),
            )
            .unwrap();

        context.last_soc_reception_time_us = 1000;
        context.soc_timeout_check_active = true;
        context
            .nmt_state_machine
            .set_state(NmtState::NmtOperational);

        // PRIME THE PUMP:
        // Force DLL state machine into a state that monitors SoC (e.g., WaitSoc or WaitPreq).
        // Sending a SocTrig event simulates receiving a valid SoC.
        context
            .dll_state_machine
            .process_event(DllCsEvent::Soc, NmtState::NmtOperational);

        context.next_tick_us = Some(2100);

        for i in 0..20 {
            context.next_tick_us = Some(2100 + (i * 1000));
            process_tick(&mut context, 2100 + (i * 1000));
            if context.error_status_changed {
                break;
            }
        }

        assert!(
            context.error_status_changed,
            "SoC timeout failed to signal error"
        );
    }

    #[test]
    fn test_heartbeat_alive() {
        let mut context = create_context();
        context.heartbeat_consumers.insert(NodeId(240), (1000, 0));
        context
            .nmt_state_machine
            .set_state(NmtState::NmtPreOperational2);
        process_tick(&mut context, 100);
        context.heartbeat_consumers.insert(NodeId(240), (1000, 900));
        process_tick(&mut context, 1200);
        assert!(!context.error_status_changed);
    }
}
