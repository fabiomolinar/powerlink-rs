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
                    // Use the AsndTransport to build the response action.
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
                    // Abort is often handled internally and returned as Ok(AbortCommand),
                    // so an Err here is likely a sequence or buffer error.
                    return NodeAction::NoAction;
                }
            };
        } else if asnd_frame.destination == context.nmt_state_machine.node_id {
            pl_trace!(*context, " Received non-SDO ASnd frame: {:?}", asnd_frame);
            // Increment general AsyncRx counter for non-SDO ASnd frames
            context.core.od.increment_counter(
                constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
                constants::SUBIDX_DIAG_NMT_COUNT_ASYNC_RX,
            );
        } else {
            // ASnd not for us, but it's still an AsyncRx frame on the network.
            // We only count frames destined for us.
            return NodeAction::NoAction;
        }
    } 
    // --- Handle SoC Frame specific logic WITH SYNC HOOKS ---
    if let PowerlinkFrame::Soc(ref soc_frame) = frame {
        
        // *** Synchronization Hooks ***
        context.last_soc_net_time = soc_frame.net_time;
        context.last_soc_relative_time = soc_frame.relative_time;
        context.last_soc_arrival_time_us = current_time_us;
        // Store flags for diagnostics
        context.last_soc_ps_flag = soc_frame.flags.ps;
        context.last_soc_mc_flag = soc_frame.flags.mc;
        
        // Calculate offset between local time and received SoC time
        let local_net_time = context.time_provider.now_net_time();
        let offset_ns = local_net_time.sub_net_time(soc_frame.net_time);

        pl_trace!(*context, " SoC received at {}us. Local Time: {:?}, SoC Time: {:?}, Offset: {}ns", 
            current_time_us, local_net_time, soc_frame.net_time, offset_ns);

        // Trigger HAL adjustment
        context.time_provider.on_soc_received(soc_frame.net_time);
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
                        pl_trace!(*context, " Scheduled SoC timeout check at {}us (earlier)", deadline);
                    }
                    None => {
                        context.next_tick_us = Some(deadline);
                        pl_trace!(*context, " Scheduled SoC timeout check at {}us (first)", deadline);
                    }
                    _ => {}
                }
            } else {
                pl_warn!(*context, " Cycle Time (0x1006) is 0, cannot schedule SoC timeout.");
                context.soc_timeout_check_active = false;
            }
        } else {
            pl_warn!(*context,
                "Could not read Cycle Time (0x1006) or SoC Tolerance (0x1C14) from OD. SoC timeout check disabled."
            );
            context.soc_timeout_check_active = false;
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

    // --- Normal Frame Processing ---
    let mut nmt_event: Option<NmtEvent> = None;
    match &frame {
        PowerlinkFrame::Soc(_) => nmt_event = Some(NmtEvent::SocReceived),
        PowerlinkFrame::SoA(_) => nmt_event = Some(NmtEvent::SoAReceived),
        PowerlinkFrame::ASnd(asnd_frame)
            if asnd_frame.destination == context.nmt_state_machine.node_id
                && asnd_frame.service_id == ServiceId::NmtCommand =>
        {
            // ... [Existing NMT Command parsing logic remains unchanged] ...
            // This is an NMT command for us.
            if let Some(cmd_id_byte) = asnd_frame.payload.first() {
                // First, try to parse as an NMT State Command
                if let Ok(cmd) = NmtStateCommand::try_from(*cmd_id_byte) {
                    // This is a state transition event
                    nmt_event = Some(match cmd {
                        NmtStateCommand::StartNode => NmtEvent::StartNode,
                        NmtStateCommand::StopNode => NmtEvent::StopNode,
                        NmtStateCommand::EnterPreOperational2 => NmtEvent::EnterPreOperational2,
                        NmtStateCommand::EnableReadyToOperate => NmtEvent::EnableReadyToOperate,
                        NmtStateCommand::ResetNode => NmtEvent::ResetNode,
                        NmtStateCommand::ResetCommunication => NmtEvent::ResetCommunication,
                        NmtStateCommand::ResetConfiguration => NmtEvent::ResetConfiguration,
                        NmtStateCommand::SwReset => NmtEvent::SwReset,
                    });
                // If not a state command, try to parse as an NMT Managing Command
                } else if let Ok(cmd) = NmtManagingCommand::try_from(*cmd_id_byte) {
                    match cmd {
                        NmtManagingCommand::NmtNetHostNameSet => {
                            // Spec 7.3.2.1.1 & Table 130
                            // Payload is [CmdID(1), Reserved(1), HostName(32)]
                            if asnd_frame.payload.len() >= 34 {
                                let hostname_bytes = &asnd_frame.payload[2..34];
                                // Find end of string (null terminator or end of slice)
                                let len = hostname_bytes.iter().position(|&b| b == 0).unwrap_or(32);
                                match String::from_utf8(hostname_bytes[..len].to_vec()) {
                                    Ok(hostname) => {
                                        pl_info!(*context, " [CN] Received NmtNetHostNameSet: '{}'", hostname);
                                        // Write to OD 0x1F9A
                                        if let Err(e) = context.core.od.write_internal(
                                            constants::IDX_NMT_HOST_NAME_VSTR, // 0x1F9A
                                            0,
                                            ObjectValue::VisibleString(hostname),
                                            false, // Bypass access checks for internal write
                                        ) {
                                            pl_error!(*context,
                                                "[CN] Failed to write new hostname to OD: {:?}",
                                                e
                                            );
                                        }

                                        // Spec: "CN requests an IdentRequest to itself"
                                        pl_info!(*context,
                                            "[CN] NmtNetHostNameSet: Queueing IdentRequest service."
                                        );
                                        // Use the new helper method on CnContext
                                        context.queue_nmt_service_request(
                                            NmtServiceRequest::IdentRequest,
                                            context.nmt_state_machine.node_id,
                                        );
                                    }
                                    Err(e) => {
                                        pl_error!(*context,
                                            "[CN] Failed to parse hostname from NmtNetHostNameSet: {:?}",
                                            e
                                        );
                                    }
                                }
                            } else {
                                pl_warn!(*context,
                                    "[CN] Received NmtNetHostNameSet with invalid payload length ({} bytes)",
                                    asnd_frame.payload.len()
                                );
                            }
                        }
                        NmtManagingCommand::NmtFlushArpEntry => {
                            // Spec 7.3.2.1.2 & Table 132
                            // Payload is [CmdID(1), Reserved(1), NodeID(1)]
                            if asnd_frame.payload.len() >= 3 {
                                let node_to_flush = asnd_frame.payload[2];
                                pl_info!(*context,
                                    "[CN] Received NmtFlushArpEntry for Node ID {}. (ARP cache not yet implemented).",
                                    node_to_flush
                                );
                                // TODO: Add call to cn.arp_cache.flush(node_to_flush)
                            } else {
                                pl_warn!(*context,
                                    "[CN] Received NmtFlushArpEntry with invalid payload length ({} bytes)",
                                    asnd_frame.payload.len()
                                );
                            }
                        }
                    }
                } else {
                    pl_warn!(*context, " Received unknown NMT Command ID: {:#04x}", cmd_id_byte);
                }
            }
        }
        _ => {}
    };

    if let Some(event) = nmt_event {
        context
            .nmt_state_machine
            .process_event(event, &mut context.core.od);
    }

    let dll_event = frame.dll_cn_event();
    // FIX: Only strictly expect PReq if we are in Operational state.
    // In PreOp2/ReadyToOp, the MN might not schedule PReqs yet (or might be busy with Async config),
    // and we don't want to reset to PreOp1 immediately.
    let expect_preq = context.nmt_state_machine.current_state() == NmtState::NmtOperational;

    if let Some(errors) = context
        .dll_state_machine
        .process_event(dll_event, context.nmt_state_machine.current_state(), context.nmt_state_machine.node_id, expect_preq)
    {
        for error in errors {
            pl_warn!(*context, " DLL state machine reported error: {:?}", error);
            // Increment history write counter for every error handled
            context.core.od.increment_counter(
                constants::IDX_DIAG_ERR_STATISTICS_REC,
                constants::SUBIDX_DIAG_ERR_STATS_HIST_WRITE,
            );

            let (nmt_action, signaled) = context.dll_error_manager.handle_error(error);
            if signaled {
                context.error_status_changed = true;
                let current_err_reg = context
                    .core
                    .od
                    .read_u8(constants::IDX_NMT_ERROR_REGISTER_U8, 0)
                    .unwrap_or(0);
                let new_err_reg = current_err_reg | 0b1;
                if current_err_reg != new_err_reg {
                    // Only increment if the value actually changed
                    context.core.od.increment_counter(
                        constants::IDX_DIAG_ERR_STATISTICS_REC,
                        constants::SUBIDX_DIAG_ERR_STATS_STATIC_ERR_CHG,
                    );
                }
                if let Err(e) = context.core.od.write_internal(
                    constants::IDX_NMT_ERROR_REGISTER_U8,
                    0,
                    crate::od::ObjectValue::Unsigned8(new_err_reg),
                    false,
                ) {
                    pl_error!(*context, " [CN] Failed to update Error Register: {:?}", e)
                }

                let error_entry = ErrorEntry {
                    entry_type: EntryType {
                        is_status_entry: false,
                        send_to_queue: true,
                        mode: ErrorEntryMode::EventOccurred,
                        profile: 0x002,
                        // FIXME: Missing profile field in ErrorEntry
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
                    // *** NEW: Write to Error History OD ***
                    error_history::write_error_to_history(&mut context.core.od, &error_entry);
                    
                    pl_info!(*context, " [CN] New error queued: {:?}", error_entry);
                    // Increment emergency write counter
                    context.core.od.increment_counter(
                        constants::IDX_DIAG_ERR_STATISTICS_REC,
                        constants::SUBIDX_DIAG_ERR_STATS_EMCY_WRITE,
                    );
                } else {
                    pl_warn!(*context,
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
            if nmt_action != NmtAction::None {
                pl_info!(*context, " DLL error triggered NMT action: {:?}", nmt_action);
                context
                    .nmt_state_machine
                    .process_event(NmtEvent::Error, &mut context.core.od);
                context.soc_timeout_check_active = false;
                return NodeAction::NoAction; // Skip response if reset
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

    // --- Error Signaling Flag Toggle ---
    if context.error_status_changed {
        context.en_flag = !context.en_flag;
        context.error_status_changed = false;
        pl_info!(*context,
            "New error detected or acknowledged, toggling EN flag to: {}",
            context.en_flag
        );
        // Increment EN flag toggle counter
        context.core.od.increment_counter(
            constants::IDX_DIAG_ERR_STATISTICS_REC,
            constants::SUBIDX_DIAG_ERR_STATS_EN_EDGE,
        );
    }

    // --- Generate Response ---
    let current_nmt_state = context.nmt_state_machine.current_state();
    let response_frame_opt = if current_nmt_state >= NmtState::NmtNotActive {
        match &frame {
            PowerlinkFrame::SoA(soa_frame) => {
                if soa_frame.target_node_id == context.nmt_state_machine.node_id {
                    match current_nmt_state {
                        NmtState::NmtPreOperational1
                        | NmtState::NmtPreOperational2
                        | NmtState::NmtReadyToOperate
                        | NmtState::NmtOperational
                        | NmtState::NmtCsStopped => match soa_frame.req_service_id {
                            RequestedServiceId::IdentRequest => {
                                // *** INCREMENT ASYNC TX COUNTER ***
                                context.core.od.increment_counter(
                                    constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
                                    constants::SUBIDX_DIAG_NMT_COUNT_ASYNC_TX,
                                );
                                Some(payload::build_ident_response(
                                    context.core.mac_address,
                                    context.nmt_state_machine.node_id,
                                    &context.core.od,
                                    soa_frame,
                                    &context.core.sdo_client,
                                    &context.pending_nmt_requests,
                                ))
                            }
                            RequestedServiceId::StatusRequest => {
                                // *** INCREMENT ASYNC TX COUNTER ***
                                context.core.od.increment_counter(
                                    constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
                                    constants::SUBIDX_DIAG_NMT_COUNT_ASYNC_TX,
                                );
                                Some(payload::build_status_response(
                                    context.core.mac_address,
                                    context.nmt_state_machine.node_id,
                                    &mut context.core.od,
                                    context.en_flag,
                                    context.ec_flag,
                                    &mut context.emergency_queue,
                                    soa_frame,
                                    &context.core.sdo_client,
                                    &context.pending_nmt_requests,
                                ))
                            }
                            RequestedServiceId::NmtRequestInvite => {
                                context.pending_nmt_requests.pop().map(|(cmd_type, tgt)| {
                                    // *** INCREMENT ASYNC TX COUNTER ***
                                    context.core.od.increment_counter(
                                        constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
                                        constants::SUBIDX_DIAG_NMT_COUNT_ASYNC_TX,
                                    );
                                    payload::build_nmt_request(
                                        context.core.mac_address,
                                        context.nmt_state_machine.node_id,
                                        cmd_type.as_u8(), // Send the raw u8 ID
                                        tgt,
                                        soa_frame,
                                    )
                                })
                            }
                            RequestedServiceId::UnspecifiedInvite => context
                                .core
                                .sdo_client
                                .pop_pending_request()
                                .map(|sdo_payload| {
                                    // *** INCREMENT SDO TX COUNTER (ASnd Request) ***
                                    context.core.od.increment_counter(
                                        constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
                                        constants::SUBIDX_DIAG_NMT_COUNT_SDO_TX,
                                    );
                                    PowerlinkFrame::ASnd(ASndFrame::new(
                                        context.core.mac_address,
                                        soa_frame.eth_header.source_mac,
                                        NodeId(C_ADR_MN_DEF_NODE_ID),
                                        context.nmt_state_machine.node_id,
                                        ServiceId::Sdo,
                                        sdo_payload.1,
                                    ))
                                }),
                            RequestedServiceId::NoService => None,
                        },
                        _ => None,
                    }
                } else {
                    None
                }
            }
            PowerlinkFrame::PReq(preq_frame) => {
                if preq_frame.destination == context.nmt_state_machine.node_id {
                    match current_nmt_state {
                        NmtState::NmtPreOperational2
                        | NmtState::NmtReadyToOperate
                        | NmtState::NmtOperational => {
                            // Increment Isochronous Tx counter
                            context.core.od.increment_counter(
                                constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
                                constants::SUBIDX_DIAG_NMT_COUNT_ISOCHR_TX,
                            );
                            Some(payload::build_pres_response(context, context.en_flag))
                        }
                        _ => None,
                    }
                } else {
                    None
                }
            }
            _ => None,
        }
    } else {
        None
    };

    // --- Serialize and return action ---
    if let Some(response_frame) = response_frame_opt {
        match serialize_frame_action(response_frame, context) {
            Ok(action) => return action,
            Err(e) => {
                pl_error!(*context, " Failed to prepare response action: {:?}", e);
                return NodeAction::NoAction;
            }
        }
    }

    NodeAction::NoAction
}