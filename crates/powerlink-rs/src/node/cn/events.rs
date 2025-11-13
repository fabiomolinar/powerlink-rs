// crates/powerlink-rs/src/node/cn/events.rs
use super::payload;
use super::state::CnContext;
use crate::common::NetTime;
use crate::frame::error::{EntryType, ErrorEntry, ErrorEntryMode};
use crate::frame::{
    ASndFrame, DllCsEvent, DllError, NmtAction, PowerlinkFrame, RequestedServiceId, ServiceId,
};
use crate::nmt::events::{CnNmtRequest, NmtEvent};
use crate::nmt::state_machine::NmtStateMachine;
use crate::nmt::states::NmtState;
use crate::node::{NodeAction, PdoHandler, serialize_frame_action};
use crate::od::constants; // Import the new constants module
use crate::sdo::server::SdoClientInfo;
use crate::sdo::transport::SdoTransport;
use crate::types::{C_ADR_MN_DEF_NODE_ID, NodeId};
// --- NEW/MODIFIED IMPORTS ---
use crate::nmt::events::{NmtManagingCommand, NmtServiceRequest, NmtStateCommand};
use crate::od::ObjectValue;
use alloc::string::String;
// --- END IMPORTS ---
use alloc::vec::Vec; // Import Vec
use log::{debug, error, info, trace, warn};

/// Processes a deserialized `PowerlinkFrame`.
pub(super) fn process_frame(
    context: &mut CnContext,
    frame: PowerlinkFrame,
    current_time_us: u64,
) -> NodeAction {
    // --- Special handling for SDO ASnd frames ---
    // (This is handled in main.rs's process_raw_frame/process_udp_datagram
    // to increment SdoRx counters before passing to SdoServer)
    if let PowerlinkFrame::ASnd(ref asnd_frame) = frame {
        if asnd_frame.destination == context.nmt_state_machine.node_id
            && asnd_frame.service_id == ServiceId::Sdo
        {
            // *** INCREMENT SDO RX COUNTER (ASnd) ***
            context.core.od.increment_counter(
                constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
                constants::SUBIDX_DIAG_NMT_COUNT_SDO_RX,
            );
            // SDO Rx logic is in main.rs, which has already incremented SdoRx.
            // We just need to handle the SDO Server logic here.
            debug!("Received SDO/ASnd frame for processing.");
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
                            // *** INCREMENT SDO TX COUNTER (ASnd Response) ***
                            context.core.od.increment_counter(
                                constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
                                constants::SUBIDX_DIAG_NMT_COUNT_SDO_TX,
                            );
                            return action;
                        }
                        Err(e) => {
                            error!("Failed to build SDO/ASnd response: {:?}", e);
                            return NodeAction::NoAction;
                        }
                    }
                }
                Err(e) => {
                    error!("SDO server error (ASnd): {:?}", e);
                    // Abort is often handled internally and returned as Ok(AbortCommand),
                    // so an Err here is likely a sequence or buffer error.
                    return NodeAction::NoAction;
                }
            };
        } else if asnd_frame.destination == context.nmt_state_machine.node_id {
            trace!("Received non-SDO ASnd frame: {:?}", asnd_frame);
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

    // --- Handle SoC Frame specific logic ---
    if let PowerlinkFrame::Soc(_) = &frame {
        trace!("SoC received at time {}", current_time_us);
        context.last_soc_reception_time_us = current_time_us;
        context.soc_timeout_check_active = true;

        // Increment Isochronous Cycle counter
        context.core.od.increment_counter(
            constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
            constants::SUBIDX_DIAG_NMT_COUNT_ISOCHR_CYC,
        );

        if context.dll_error_manager.on_cycle_complete() {
            info!("[CN] All DLL errors cleared, resetting Generic Error bit.");
            let current_err_reg = context
                .core
                .od
                .read_u8(constants::IDX_NMT_ERROR_REGISTER_U8, 0)
                .unwrap_or(0);
            let new_err_reg = current_err_reg & !0b1;
            context
                .core
                .od
                .write_internal(
                    constants::IDX_NMT_ERROR_REGISTER_U8,
                    0,
                    crate::od::ObjectValue::Unsigned8(new_err_reg),
                    false,
                )
                .unwrap_or_else(|e| error!("[CN] Failed to clear Error Register: {:?}", e));
            context.error_status_changed = true;
            // Increment Static Error Bit Field Changed counter
            context.core.od.increment_counter(
                constants::IDX_DIAG_ERR_STATISTICS_REC,
                constants::SUBIDX_DIAG_ERR_STATS_STATIC_ERR_CHG,
            );
        }

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
                        trace!("Scheduled SoC timeout check at {}us (earlier)", deadline);
                    }
                    None => {
                        context.next_tick_us = Some(deadline);
                        trace!("Scheduled SoC timeout check at {}us (first)", deadline);
                    }
                    _ => {}
                }
            } else {
                warn!("Cycle Time (0x1006) is 0, cannot schedule SoC timeout.");
                context.soc_timeout_check_active = false;
            }
        } else {
            warn!(
                "Could not read Cycle Time (0x1006) or SoC Tolerance (0x1C14) from OD. SoC timeout check disabled."
            );
            context.soc_timeout_check_active = false;
        }
    }

    // Increment Isochronous/Asynchronous Rx counters for other frames
    match &frame {
        PowerlinkFrame::PReq(_) => {
            context.core.od.increment_counter(
                constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
                constants::SUBIDX_DIAG_NMT_COUNT_ISOCHR_RX,
            );
        }
        PowerlinkFrame::PRes(pres_frame) => {
            // Count PRes cross-traffic
            context.core.od.increment_counter(
                constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
                constants::SUBIDX_DIAG_NMT_COUNT_ISOCHR_RX,
            );
            // --- Heartbeat Consumer Check ---
            // A PRes frame is a heartbeat for its source node.
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
        _ => {} // SoC and ASnd already handled
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
                    if preq.flags.ea == context.en_flag {
                        trace!(
                            "Received matching EA flag ({}) from MN in PReq.",
                            preq.flags.ea
                        );
                    } else {
                        trace!(
                            "Received mismatched EA flag ({}, EN is {}) from MN in PReq.",
                            preq.flags.ea, context.en_flag
                        );
                    }
                }
            }
            PowerlinkFrame::SoA(soa) => {
                if soa.target_node_id == context.nmt_state_machine.node_id {
                    if soa.flags.er {
                        info!(
                            "Received ER flag from MN in SoA, resetting EN flag and Emergency Queue."
                        );
                        context.en_flag = false;
                        context.emergency_queue.clear();
                        // Increment ER counter
                        context.core.od.increment_counter(
                            constants::IDX_DIAG_ERR_STATISTICS_REC,
                            constants::SUBIDX_DIAG_ERR_STATS_ER_POS_EDGE,
                        );
                    }
                    context.ec_flag = soa.flags.er;
                    trace!(
                        "Processed SoA flags: ER={}, EC set to {}",
                        soa.flags.er, context.ec_flag
                    );
                    if soa.flags.ea == context.en_flag {
                        trace!(
                            "Received matching EA flag ({}) from MN in SoA.",
                            soa.flags.ea
                        );
                    } else {
                        trace!(
                            "Received mismatched EA flag ({}, EN is {}) from MN in SoA.",
                            soa.flags.ea, context.en_flag
                        );
                    }
                }
            }
            _ => {}
        }
    }

    // --- Normal Frame Processing ---
    let mut nmt_event: Option<NmtEvent> = None;
    match &frame {
        PowerlinkFrame::Soc(_) => nmt_event = Some(NmtEvent::SocReceived),
        PowerlinkFrame::SoA(_) => nmt_event = Some(NmtEvent::SocSoAReceived),
        PowerlinkFrame::ASnd(asnd_frame)
            if asnd_frame.destination == context.nmt_state_machine.node_id
                && asnd_frame.service_id == ServiceId::NmtCommand =>
        {
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
                                        info!("[CN] Received NmtNetHostNameSet: '{}'", hostname);
                                        // Write to OD 0x1F9A
                                        if let Err(e) = context.core.od.write_internal(
                                            constants::IDX_NMT_HOST_NAME_VSTR, // 0x1F9A
                                            0,
                                            ObjectValue::VisibleString(hostname),
                                            false, // Bypass access checks for internal write
                                        ) {
                                            error!("[CN] Failed to write new hostname to OD: {:?}", e);
                                        }

                                        // Spec: "CN requests an IdentRequest to itself"
                                        info!("[CN] NmtNetHostNameSet: Queueing IdentRequest service.");
                                        context.pending_nmt_requests.push((
                                            CnNmtRequest::Service(NmtServiceRequest::IdentRequest),
                                            context.nmt_state_machine.node_id,
                                        ));
                                    }
                                    Err(e) => {
                                        error!("[CN] Failed to parse hostname from NmtNetHostNameSet: {:?}", e);
                                    }
                                }
                            } else {
                                warn!("[CN] Received NmtNetHostNameSet with invalid payload length ({} bytes)", asnd_frame.payload.len());
                            }
                        }
                        NmtManagingCommand::NmtFlushArpEntry => {
                            // Spec 7.3.2.1.2 & Table 132
                            // Payload is [CmdID(1), Reserved(1), NodeID(1)]
                            if asnd_frame.payload.len() >= 3 {
                                let node_to_flush = asnd_frame.payload[2];
                                info!("[CN] Received NmtFlushArpEntry for Node ID {}. (ARP cache not yet implemented).", node_to_flush);
                                // TODO: Add call to cn.arp_cache.flush(node_to_flush)
                            } else {
                                warn!("[CN] Received NmtFlushArpEntry with invalid payload length ({} bytes)", asnd_frame.payload.len());
                            }
                        }
                    }
                } else {
                    warn!("Received unknown NMT Command ID: {:#04x}", cmd_id_byte);
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
    if let Some(errors) = context
        .dll_state_machine
        .process_event(dll_event, context.nmt_state_machine.current_state())
    {
        for error in errors {
            warn!("DLL state machine reported error: {:?}", error);
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
                context
                    .core
                    .od
                    .write_internal(
                        constants::IDX_NMT_ERROR_REGISTER_U8,
                        0,
                        crate::od::ObjectValue::Unsigned8(new_err_reg),
                        false,
                    )
                    .unwrap_or_else(|e| error!("[CN] Failed to update Error Register: {:?}", e));

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
                    context.emergency_queue.push_back(error_entry);
                    info!("[CN] New error queued: {:?}", error_entry);
                    // Increment emergency write counter
                    context.core.od.increment_counter(
                        constants::IDX_DIAG_ERR_STATISTICS_REC,
                        constants::SUBIDX_DIAG_ERR_STATS_EMCY_WRITE,
                    );
                } else {
                    warn!(
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
                info!("DLL error triggered NMT action: {:?}", nmt_action);
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
        info!(
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
                error!("Failed to prepare response action: {:?}", e);
                return NodeAction::NoAction;
            }
        }
    }

    NodeAction::NoAction
}

/// Processes a timeout or other periodic check.
pub(super) fn process_tick(context: &mut CnContext, current_time_us: u64) -> NodeAction {
    // --- SDO Server Tick (handles timeouts/retransmissions) ---
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
            match build_result {
                Ok(action) => return action,
                Err(e) => error!("[CN] Failed to build SDO Abort frame: {:?}", e),
            }
            // If building the abort frame failed, fall through to other tick logic.
        }
        Err(e) => error!("[CN] SDO Server tick error: {:?}", e),
        _ => {} // No action or no error
    }

    let current_nmt_state = context.nmt_state_machine.current_state();

    // --- Heartbeat Consumer Check ---
    if current_nmt_state >= NmtState::NmtPreOperational2 {
        let mut timed_out_nodes = Vec::new();
        for (node_id, (timeout_us, last_seen_us)) in &mut context.heartbeat_consumers {
            if *last_seen_us == 0 {
                // First tick in a valid state, initialize last_seen_us to now
                *last_seen_us = current_time_us;
            } else if *timeout_us > 0 && (current_time_us - *last_seen_us > *timeout_us) {
                // Timeout detected!
                warn!(
                    "[CN] Heartbeat timeout for Node {}! Last seen {}us ago (timeout is {}us).",
                    node_id.0,
                    current_time_us - *last_seen_us,
                    *timeout_us
                );
                timed_out_nodes.push(*node_id);
                // Reset last_seen to prevent continuous error reporting every tick
                *last_seen_us = current_time_us;
            }
        }

        // Handle errors outside the mutable borrow
        for node_id in timed_out_nodes {
            // We report the error, which will trigger the threshold counter
            let (nmt_action, signaled) = context
                .dll_error_manager
                .handle_error(DllError::HeartbeatTimeout { node_id });

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
                    // Increment static error change counter
                    context.core.od.increment_counter(
                        constants::IDX_DIAG_ERR_STATISTICS_REC,
                        constants::SUBIDX_DIAG_ERR_STATS_STATIC_ERR_CHG,
                    );
                }
                context
                    .core
                    .od
                    .write_internal(
                        constants::IDX_NMT_ERROR_REGISTER_U8,
                        0,
                        crate::od::ObjectValue::Unsigned8(new_err_reg),
                        false,
                    )
                    .unwrap_or_else(|e| error!("[CN] Failed to update Error Register: {:?}", e));
            }
            if nmt_action != NmtAction::None {
                context
                    .nmt_state_machine
                    .process_event(NmtEvent::Error, &mut context.core.od);
                context.soc_timeout_check_active = false;
                // If an NMT reset is triggered, stop further tick processing.
                return NodeAction::NoAction;
            }
        }
    }

    // Check if a deadline is set and if it has passed
    let deadline_passed = context
        .next_tick_us
        .is_some_and(|deadline| current_time_us >= deadline);

    // --- Handle NmtNotActive Timeout Setup ---
    if current_nmt_state == NmtState::NmtNotActive && context.next_tick_us.is_none() {
        let timeout_us = context.nmt_state_machine.basic_ethernet_timeout as u64;
        if timeout_us > 0 {
            let deadline = current_time_us + timeout_us;
            context.next_tick_us = Some(deadline);
            debug!(
                "No SoC/SoA seen, starting BasicEthernet timeout check ({}us). Deadline: {}us",
                timeout_us, deadline
            );
        } else {
            debug!("BasicEthernet timeout is 0, check disabled.");
        }
        return NodeAction::NoAction; // Don't act on this first call, just set the timer.
    }

    // If no deadline has passed, do nothing else this tick.
    if !deadline_passed {
        return NodeAction::NoAction;
    }

    // --- A deadline has passed ---
    trace!(
        "Tick deadline reached at {}us (Deadline was {:?})",
        current_time_us, context.next_tick_us
    );
    context.next_tick_us = None; // Consume the deadline

    // --- Handle Specific Timeouts ---
    // NmtNotActive -> BasicEthernet
    if current_nmt_state == NmtState::NmtNotActive {
        let timeout_us = context.nmt_state_machine.basic_ethernet_timeout as u64;
        if timeout_us > 0 {
            warn!("BasicEthernet timeout expired. Transitioning state.");
            context
                .nmt_state_machine
                .process_event(NmtEvent::Timeout, &mut context.core.od);
            context.soc_timeout_check_active = false;
        }
        return NodeAction::NoAction; // No further action this tick
    }
    // SoC Timeout Check
    else if context.soc_timeout_check_active {
        warn!(
            "SoC timeout detected at {}us! Last SoC was at {}us.",
            current_time_us, context.last_soc_reception_time_us
        );
        if let Some(errors) = context
            .dll_state_machine
            .process_event(DllCsEvent::SocTimeout, current_nmt_state)
        {
            for error in errors {
                // Increment history write counter
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
                        // Increment static error change counter
                        context.core.od.increment_counter(
                            constants::IDX_DIAG_ERR_STATISTICS_REC,
                            constants::SUBIDX_DIAG_ERR_STATS_STATIC_ERR_CHG,
                        );
                    }
                    context
                        .core
                        .od
                        .write_internal(
                            constants::IDX_NMT_ERROR_REGISTER_U8,
                            0,
                            crate::od::ObjectValue::Unsigned8(new_err_reg),
                            false,
                        )
                        .unwrap_or_else(|e| {
                            error!("[CN] Failed to update Error Register: {:?}", e)
                        });
                }
                if nmt_action != NmtAction::None {
                    context
                        .nmt_state_machine
                        .process_event(NmtEvent::Error, &mut context.core.od);
                    context.soc_timeout_check_active = false;
                    return NodeAction::NoAction; // Stop processing after NMT reset
                }
            }
        }
        // Reschedule next check if still active
        if context.soc_timeout_check_active {
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
                    let cycles_missed = ((current_time_us - context.last_soc_reception_time_us)
                        / cycle_time_us)
                        + 1;
                    let next_expected_soc_time =
                        context.last_soc_reception_time_us + cycles_missed * cycle_time_us;
                    let next_deadline = next_expected_soc_time + (tolerance_ns / 1000);
                    context.next_tick_us = Some(next_deadline);
                    trace!(
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
        trace!(
            "Tick deadline reached, but no specific timeout active (State: {:?}).",
            current_nmt_state
        );
    }

    NodeAction::NoAction // Default return if no frame needs sending
}