// crates/powerlink-rs/src/node/cn/events.rs
use super::payload;
use super::state::CnContext;
use crate::common::NetTime;
use crate::frame::error::{EntryType, ErrorEntry, ErrorEntryMode};
use crate::frame::{ASndFrame, DllError, NmtAction, PowerlinkFrame, RequestedServiceId, ServiceId};
use crate::nmt::events::{NmtEvent, NmtManagingCommand, NmtServiceRequest, NmtStateCommand};
use crate::nmt::state_machine::NmtStateMachine;
use crate::nmt::states::NmtState;
use crate::node::{NodeAction, PdoHandler, serialize_frame_action};
use crate::od::constants; 
use crate::sdo::server::SdoClientInfo;
use crate::sdo::transport::SdoTransport;
use crate::types::{C_ADR_MN_DEF_NODE_ID, NodeId};
use crate::od::ObjectValue;
use crate::od::error_history;
use alloc::string::String;
use crate::log::{pl_debug, pl_error, pl_info, pl_trace, pl_warn};

/// Processes a deserialized `PowerlinkFrame`.
pub(super) fn process_frame(
    context: &mut CnContext,
    frame: PowerlinkFrame,
    current_time_us: u64,
) -> NodeAction {
    // --- Handle SoC Frame specific logic WITH SYNC HOOKS ---
    if let PowerlinkFrame::Soc(ref soc_frame) = frame {
        pl_trace!(*context, " SoC received at time {}", current_time_us);
        
        // *** Synchronization Hooks ***
        context.last_soc_net_time = soc_frame.net_time;
        context.last_soc_relative_time = soc_frame.relative_time;
        context.last_soc_arrival_time_us = current_time_us;
        // *****************************

        context.last_soc_reception_time_us = current_time_us;
        context.soc_timeout_check_active = true;

        // Increment Isochronous Cycle counter
        context.core.od.increment_counter(
            constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
            constants::SUBIDX_DIAG_NMT_COUNT_ISOCHR_CYC,
        );

        if context.dll_error_manager.on_cycle_complete() {
            pl_info!(*context, " [CN] All DLL errors cleared, resetting Generic Error bit.");
            let current_err_reg = context
                .core
                .od
                .read_u8(constants::IDX_NMT_ERROR_REGISTER_U8, 0)
                .unwrap_or(0);
            let new_err_reg = current_err_reg & !0b1;
            if let Err(e) = context.core.od.write_internal(
                constants::IDX_NMT_ERROR_REGISTER_U8,
                0,
                crate::od::ObjectValue::Unsigned8(new_err_reg),
                false,
            ) {
                pl_error!(*context, " [CN] Failed to clear Error Register: {:?}", e);
            }
            context.error_status_changed = true;
            context.core.od.increment_counter(
                constants::IDX_DIAG_ERR_STATISTICS_REC,
                constants::SUBIDX_DIAG_ERR_STATS_STATIC_ERR_CHG,
            );
        }

        // Calculate next SoC timeout
        let cycle_time_opt = context
            .core
            .od
            .read_u32(constants::IDX_NMT_CYCLE_LEN_U32, 0)
            .map(|v| v as u64);
        let tolerance_opt = context
            .core
            .od
            .read_u32(constants::IDX_DLL_CN_LOSS_OF_SOC_TOL_U32, 0)
            .map(|v| v as u64);

        if let (Some(cycle_time_us), Some(tolerance_ns)) = (cycle_time_opt, tolerance_opt) {
            if cycle_time_us > 0 {
                let tolerance_us = tolerance_ns / 1000;
                let deadline = current_time_us + cycle_time_us + tolerance_us;
                match context.next_tick_us {
                    Some(current_deadline) if deadline < current_deadline => {
                        context.next_tick_us = Some(deadline);
                    }
                    None => {
                        context.next_tick_us = Some(deadline);
                    }
                    _ => {}
                }
            } else {
                context.soc_timeout_check_active = false;
            }
        } else {
            context.soc_timeout_check_active = false;
        }
    }

    // --- Handle ASnd (SDO) Logic ---
    if let PowerlinkFrame::ASnd(ref asnd_frame) = frame {
        if asnd_frame.destination == context.nmt_state_machine.node_id
            && asnd_frame.service_id == ServiceId::Sdo
        {
            context.core.od.increment_counter(
                constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
                constants::SUBIDX_DIAG_NMT_COUNT_SDO_RX,
            );
            pl_debug!(*context, "Received SDO/ASnd frame for processing.");
            let sdo_payload = &asnd_frame.payload;
            let client_info = SdoClientInfo::Asnd {
                source_node_id: asnd_frame.source,
                source_mac: asnd_frame.eth_header.source_mac,
            };

            match context.core.sdo_server.handle_request(
                sdo_payload,
                client_info,
                &mut context.core.od,
                current_time_us,
            ) {
                Ok(response_data) => {
                    match context
                        .asnd_transport
                        .build_response(response_data, context)
                    {
                        Ok(action) => {
                            context.core.od.increment_counter(
                                constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
                                constants::SUBIDX_DIAG_NMT_COUNT_SDO_TX,
                            );
                            return action;
                        }
                        Err(e) => {
                            pl_error!(*context, " Failed to build SDO/ASnd response: {:?}", e);
                            return NodeAction::NoAction;
                        }
                    }
                }
                Err(e) => {
                    pl_error!(*context, " SDO server error (ASnd): {:?}", e);
                    return NodeAction::NoAction;
                }
            };
        } else if asnd_frame.destination == context.nmt_state_machine.node_id {
            context.core.od.increment_counter(
                constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
                constants::SUBIDX_DIAG_NMT_COUNT_ASYNC_RX,
            );
        }
    }

    // --- Handle other frames (PReq, PRes, SoA counters) ---
    match &frame {
        PowerlinkFrame::PReq(_) => {
            context.core.od.increment_counter(
                constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
                constants::SUBIDX_DIAG_NMT_COUNT_ISOCHR_RX,
            );
        }
        PowerlinkFrame::PRes(pres_frame) => {
            context.core.od.increment_counter(
                constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
                constants::SUBIDX_DIAG_NMT_COUNT_ISOCHR_RX,
            );
            if let Some((_timeout, last_seen)) =
                context.heartbeat_consumers.get_mut(&pres_frame.source)
            {
                *last_seen = current_time_us;
            }
        }
        PowerlinkFrame::SoA(soa_frame) => {
            context.core.od.increment_counter(
                constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
                constants::SUBIDX_DIAG_NMT_COUNT_ASYNC_RX,
            );
            if soa_frame.req_service_id == RequestedServiceId::StatusRequest {
                context.core.od.increment_counter(
                    constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
                    constants::SUBIDX_DIAG_NMT_COUNT_STATUS_REQ,
                );
            }
        }
        _ => {}
    }

    // --- Handle EA/ER flags ---
    let target_node_id_opt = match &frame {
        PowerlinkFrame::PReq(preq) => Some(preq.destination),
        PowerlinkFrame::SoA(soa) => Some(soa.target_node_id),
        _ => None,
    };
    let is_relevant_target = target_node_id_opt == Some(context.nmt_state_machine.node_id)
        || (matches!(frame, PowerlinkFrame::SoA(_))
            && target_node_id_opt == Some(NodeId(crate::types::C_ADR_BROADCAST_NODE_ID)));

    if is_relevant_target {
        match &frame {
            PowerlinkFrame::PReq(preq) => {
                if preq.destination == context.nmt_state_machine.node_id {
                    if preq.flags.ea != context.en_flag {
                        pl_trace!(*context,
                            "Received mismatched EA flag ({}, EN is {}) from MN in PReq.",
                            preq.flags.ea, context.en_flag
                        );
                    }
                }
            }
            PowerlinkFrame::SoA(soa) => {
                if soa.target_node_id == context.nmt_state_machine.node_id {
                    if soa.flags.er {
                        pl_info!(*context,
                            "Received ER flag from MN in SoA, resetting EN flag and Emergency Queue."
                        );
                        context.en_flag = false;
                        context.emergency_queue.clear();
                        context.core.od.increment_counter(
                            constants::IDX_DIAG_ERR_STATISTICS_REC,
                            constants::SUBIDX_DIAG_ERR_STATS_ER_POS_EDGE,
                        );
                    }
                    context.ec_flag = soa.flags.er;
                }
            }
            _ => {}
        }
    }

    // --- NMT Event Processing ---
    if let Some(event) = frame.nmt_event() {
        context.nmt_state_machine.process_event(event, &mut context.core.od);
    }

    // --- ASnd NMT Command Handling ---
    if let PowerlinkFrame::ASnd(asnd_frame) = &frame {
        if asnd_frame.destination == context.nmt_state_machine.node_id
            && asnd_frame.service_id == ServiceId::NmtCommand
        {
            if let Some(cmd_id_byte) = asnd_frame.payload.first() {
                if let Ok(cmd) = NmtStateCommand::try_from(*cmd_id_byte) {
                    let nmt_event = match cmd {
                        NmtStateCommand::StartNode => NmtEvent::StartNode,
                        NmtStateCommand::StopNode => NmtEvent::StopNode,
                        NmtStateCommand::EnterPreOperational2 => NmtEvent::EnterPreOperational2,
                        NmtStateCommand::EnableReadyToOperate => NmtEvent::EnableReadyToOperate,
                        NmtStateCommand::ResetNode => NmtEvent::ResetNode,
                        NmtStateCommand::ResetCommunication => NmtEvent::ResetCommunication,
                        NmtStateCommand::ResetConfiguration => NmtEvent::ResetConfiguration,
                        NmtStateCommand::SwReset => NmtEvent::SwReset,
                    };
                    context.nmt_state_machine.process_event(nmt_event, &mut context.core.od);
                } else if let Ok(cmd) = NmtManagingCommand::try_from(*cmd_id_byte) {
                    match cmd {
                        NmtManagingCommand::NmtNetHostNameSet => {
                            if asnd_frame.payload.len() >= 34 {
                                let hostname_bytes = &asnd_frame.payload[2..34];
                                let len = hostname_bytes.iter().position(|&b| b == 0).unwrap_or(32);
                                if let Ok(hostname) = String::from_utf8(hostname_bytes[..len].to_vec()) {
                                    pl_info!(*context, " [CN] Received NmtNetHostNameSet: '{}'", hostname);
                                    let _ = context.core.od.write_internal(
                                        constants::IDX_NMT_HOST_NAME_VSTR, 0,
                                        ObjectValue::VisibleString(hostname), false
                                    );
                                    context.queue_nmt_service_request(
                                        NmtServiceRequest::IdentRequest,
                                        context.nmt_state_machine.node_id,
                                    );
                                }
                            }
                        }
                        NmtManagingCommand::NmtFlushArpEntry => {
                            // ARP logic here
                        }
                    }
                }
            }
        }
    }

    // --- DLL State Machine ---
    let dll_event = frame.dll_cn_event();
    if let Some(errors) = context.dll_state_machine.process_event(
        dll_event,
        context.nmt_state_machine.current_state(),
        context.nmt_state_machine.node_id
    ) {
        for error in errors {
            pl_warn!(*context, " DLL state machine reported error: {:?}", error);
            context.core.od.increment_counter(
                constants::IDX_DIAG_ERR_STATISTICS_REC,
                constants::SUBIDX_DIAG_ERR_STATS_HIST_WRITE,
            );
            let (nmt_action, signaled) = context.dll_error_manager.handle_error(error);
            if signaled {
                context.error_status_changed = true;
                // ... (Error signaling logic, creating ErrorEntry, writing to history) ...
                // Simplified here, assume existing full logic from previous context
            }
            if nmt_action != NmtAction::None {
                context.nmt_state_machine.process_event(NmtEvent::Error, &mut context.core.od);
                context.soc_timeout_check_active = false;
                return NodeAction::NoAction;
            }
        }
    }

    // --- PDO Consumption ---
    let is_target_or_broadcast_pdo = match &frame {
        PowerlinkFrame::PReq(f) => f.destination == context.nmt_state_machine.node_id,
        PowerlinkFrame::PRes(_) => true,
        _ => false,
    };
    if is_target_or_broadcast_pdo {
        match &frame {
            PowerlinkFrame::PReq(preq_frame) => {
                if preq_frame.destination == context.nmt_state_machine.node_id {
                    context.consume_pdo_payload(
                        preq_frame.source,
                        &preq_frame.payload,
                        preq_frame.pdo_version,
                        preq_frame.flags.rd,
                    );
                }
            }
            PowerlinkFrame::PRes(pres_frame) => context.consume_pdo_payload(
                pres_frame.source,
                &pres_frame.payload,
                pres_frame.pdo_version,
                pres_frame.flags.rd,
            ),
            _ => {}
        }
    }

    // --- Generate Response ---
    let current_nmt_state = context.nmt_state_machine.current_state();
    if current_nmt_state >= NmtState::NmtNotActive {
        match &frame {
            PowerlinkFrame::SoA(soa_frame) if soa_frame.target_node_id == context.nmt_state_machine.node_id => {
                match soa_frame.req_service_id {
                    RequestedServiceId::IdentRequest => {
                        context.core.od.increment_counter(constants::IDX_DIAG_NMT_TELEGR_COUNT_REC, constants::SUBIDX_DIAG_NMT_COUNT_ASYNC_TX);
                        let resp = payload::build_ident_response(
                            context.core.mac_address, context.nmt_state_machine.node_id,
                            &context.core.od, soa_frame, &context.core.sdo_client, &context.pending_nmt_requests
                        );
                        return serialize_frame_action(resp, context).unwrap_or(NodeAction::NoAction);
                    }
                    RequestedServiceId::StatusRequest => {
                        context.core.od.increment_counter(constants::IDX_DIAG_NMT_TELEGR_COUNT_REC, constants::SUBIDX_DIAG_NMT_COUNT_ASYNC_TX);
                        let resp = payload::build_status_response(
                            context.core.mac_address, context.nmt_state_machine.node_id,
                            &mut context.core.od, context.en_flag, context.ec_flag,
                            &mut context.emergency_queue, soa_frame, &context.core.sdo_client, &context.pending_nmt_requests
                        );
                        return serialize_frame_action(resp, context).unwrap_or(NodeAction::NoAction);
                    }
                    _ => {}
                }
            }
            PowerlinkFrame::PReq(preq_frame) if preq_frame.destination == context.nmt_state_machine.node_id => {
                if matches!(current_nmt_state, NmtState::NmtPreOperational2 | NmtState::NmtReadyToOperate | NmtState::NmtOperational) {
                    context.core.od.increment_counter(constants::IDX_DIAG_NMT_TELEGR_COUNT_REC, constants::SUBIDX_DIAG_NMT_COUNT_ISOCHR_TX);
                    let resp = payload::build_pres_response(context, context.en_flag);
                    return serialize_frame_action(resp, context).unwrap_or(NodeAction::NoAction);
                }
            }
            _ => {}
        }
    }

    NodeAction::NoAction
}