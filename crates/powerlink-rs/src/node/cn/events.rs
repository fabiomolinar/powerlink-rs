// crates/powerlink-rs/src/node/cn/events.rs

use super::payload;
use super::state::CnContext;
use crate::common::NetTime;
use crate::frame::codec::CodecHelpers;
use crate::frame::error::{EntryType, ErrorEntry, ErrorEntryMode};
use crate::frame::{
    ASndFrame, DllCsEvent, DllError, NmtAction, PowerlinkFrame,
    RequestedServiceId, ServiceId,
};
use crate::nmt::events::{NmtCommand, NmtEvent};
use crate::nmt::state_machine::NmtStateMachine;
use crate::nmt::states::NmtState;
use crate::node::NodeAction;
use crate::sdo::asnd;
use crate::sdo::command::SdoCommand; // Added import
use crate::sdo::sequence::SequenceLayerHeader; // Added import
use crate::sdo::server::SdoClientInfo;
#[cfg(feature = "sdo-udp")]
use crate::sdo::udp::serialize_sdo_udp_payload;
use crate::types::{C_ADR_MN_DEF_NODE_ID, NodeId};
use crate::PowerlinkError;
use alloc::vec;
use log::{debug, error, info, trace, warn};

// Constants for OD access
const OD_IDX_CYCLE_TIME: u16 = 0x1006;
const OD_IDX_LOSS_SOC_TOLERANCE: u16 = 0x1C14;
const OD_IDX_ERROR_REGISTER: u16 = 0x1001;

/// Processes a deserialized `PowerlinkFrame`.
pub(super) fn process_frame(
    context: &mut CnContext,
    frame: PowerlinkFrame,
    current_time_us: u64,
) -> NodeAction {
    // --- Special handling for SDO ASnd frames ---
    if let PowerlinkFrame::ASnd(ref asnd_frame) = frame {
        if asnd_frame.destination == context.nmt_state_machine.node_id
            && asnd_frame.service_id == ServiceId::Sdo
        {
            debug!("Received SDO/ASnd frame for processing.");
            // SDO payload starts right after ASnd header (MType, Dest, Src, SvcID = 4 bytes)
            let sdo_payload = &asnd_frame.payload; // ASnd payload *is* the SDO payload (Seq+Cmd+Data)
            let client_info = SdoClientInfo::Asnd {
                source_node_id: asnd_frame.source,
                source_mac: asnd_frame.eth_header.source_mac,
            };

            match context.sdo_server.handle_request(
                sdo_payload,
                client_info, // Pass the client_info we just created
                &mut context.od,
                current_time_us,
            ) {
                // SdoResponseData is gone, we get the components back
                Ok((seq_header, command)) => {
                    // Pass all components to the build function
                    match build_asnd_from_sdo_response(context, client_info, seq_header, command) {
                        Ok(action) => return action,
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
        } else {
            return NodeAction::NoAction; // ASnd not for us
        }
    }

    // --- Handle SoC Frame specific logic ---
    if let PowerlinkFrame::Soc(_) = &frame {
        trace!("SoC received at time {}", current_time_us);
        context.last_soc_reception_time_us = current_time_us;
        context.soc_timeout_check_active = true;
        if context.dll_error_manager.on_cycle_complete() {
            info!("[CN] All DLL errors cleared, resetting Generic Error bit.");
            let current_err_reg = context.od.read_u8(OD_IDX_ERROR_REGISTER, 0).unwrap_or(0);
            let new_err_reg = current_err_reg & !0b1;
            context
                .od
                .write_internal(
                    OD_IDX_ERROR_REGISTER,
                    0,
                    crate::od::ObjectValue::Unsigned8(new_err_reg),
                    false,
                )
                .unwrap_or_else(|e| error!("[CN] Failed to clear Error Register: {:?}", e));
            context.error_status_changed = true;
        }

        let cycle_time_opt = context.od.read_u32(OD_IDX_CYCLE_TIME, 0).map(|v| v as u64);
        let tolerance_opt = context
            .od
            .read_u32(OD_IDX_LOSS_SOC_TOLERANCE, 0)
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
            warn!("Could not read Cycle Time (0x1006) or SoC Tolerance (0x1C14) from OD. SoC timeout check disabled.");
            context.soc_timeout_check_active = false;
        }
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
                            preq.flags.ea,
                            context.en_flag
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
                    }
                    context.ec_flag = soa.flags.er;
                    trace!(
                        "Processed SoA flags: ER={}, EC set to {}",
                        soa.flags.er,
                        context.ec_flag
                    );
                    if soa.flags.ea == context.en_flag {
                        trace!("Received matching EA flag ({}) from MN in SoA.", soa.flags.ea);
                    } else {
                        trace!(
                            "Received mismatched EA flag ({}, EN is {}) from MN in SoA.",
                            soa.flags.ea,
                            context.en_flag
                        );
                    }
                }
            }
            _ => {}
        }
    }

    // --- Normal Frame Processing ---
    let nmt_event = match &frame {
        PowerlinkFrame::Soc(_) => Some(NmtEvent::SocReceived),
        PowerlinkFrame::SoA(_) => Some(NmtEvent::SocSoAReceived),
        PowerlinkFrame::ASnd(asnd)
            if asnd.destination == context.nmt_state_machine.node_id
                && asnd.service_id == ServiceId::NmtCommand =>
        {
            asnd.payload
                .get(0)
                .and_then(|&b| NmtCommand::try_from(b).ok())
                .map(|cmd| match cmd {
                    NmtCommand::StartNode => NmtEvent::StartNode,
                    NmtCommand::StopNode => NmtEvent::StopNode,
                    NmtCommand::EnterPreOperational2 => NmtEvent::EnterPreOperational2,
                    NmtCommand::EnableReadyToOperate => NmtEvent::EnableReadyToOperate,
                    NmtCommand::ResetNode => NmtEvent::ResetNode,
                    NmtCommand::ResetCommunication => NmtEvent::ResetCommunication,
                    NmtCommand::ResetConfiguration => NmtEvent::ResetConfiguration,
                    NmtCommand::SwReset => NmtEvent::SwReset,
                })
        }
        _ => None,
    };
    if let Some(event) = nmt_event {
        context
            .nmt_state_machine
            .process_event(event, &mut context.od);
    }

    let dll_event = frame.dll_cn_event();
    if let Some(errors) = context
        .dll_state_machine
        .process_event(dll_event, context.nmt_state_machine.current_state())
    {
        for error in errors {
            warn!("DLL state machine reported error: {:?}", error);
            let (nmt_action, signaled) = context.dll_error_manager.handle_error(error);
            if signaled {
                context.error_status_changed = true;
                let current_err_reg = context.od.read_u8(OD_IDX_ERROR_REGISTER, 0).unwrap_or(0);
                let new_err_reg = current_err_reg | 0b1;
                context
                    .od
                    .write_internal(
                        OD_IDX_ERROR_REGISTER,
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
                } else {
                    warn!(
                        "[CN] Emergency queue full, dropping error: {:?}",
                        error_entry
                    );
                }
            }
            if nmt_action != NmtAction::None {
                info!("DLL error triggered NMT action: {:?}", nmt_action);
                context
                    .nmt_state_machine
                    .process_event(NmtEvent::Error, &mut context.od);
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
                    // TODO This is a temporary fix, as PdoHandler is not yet implemented for CnContext
                    // self.consume_preq_payload(preq_frame);
                }
            }
            PowerlinkFrame::PRes(_pres_frame) => {
                // TODO This is a temporary fix, as PdoHandler is not yet implemented for CnContext
                // self.consume_pres_payload(pres_frame)
            }
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
                        | NmtState::NmtCsStopped => {
                            match soa_frame.req_service_id {
                                RequestedServiceId::IdentRequest => Some(
                                    payload::build_ident_response(
                                        context.mac_address,
                                        context.nmt_state_machine.node_id,
                                        &context.od,
                                        soa_frame,
                                    ),
                                ),
                                RequestedServiceId::StatusRequest => Some(
                                    payload::build_status_response(
                                        context.mac_address,
                                        context.nmt_state_machine.node_id,
                                        &mut context.od,
                                        context.en_flag,
                                        context.ec_flag,
                                        &mut context.emergency_queue,
                                        soa_frame,
                                    ),
                                ),
                                RequestedServiceId::NmtRequestInvite => context
                                    .pending_nmt_requests
                                    .pop()
                                    .map(|(cmd, tgt)| {
                                        payload::build_nmt_request(
                                            context.mac_address,
                                            context.nmt_state_machine.node_id,
                                            cmd,
                                            tgt,
                                            soa_frame,
                                        )
                                    }),
                                RequestedServiceId::UnspecifiedInvite => {
                                    context.sdo_client.pop_pending_request().map(|sdo_payload| {
                                        PowerlinkFrame::ASnd(ASndFrame::new(
                                            context.mac_address,
                                            soa_frame.eth_header.source_mac,
                                            NodeId(C_ADR_MN_DEF_NODE_ID),
                                            context.nmt_state_machine.node_id,
                                            ServiceId::Sdo,
                                            sdo_payload.1,
                                        ))
                                    })
                                }
                                RequestedServiceId::NoService => None,
                            }
                        }
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
                        | NmtState::NmtOperational => Some(payload::build_pres_response(
                            context.mac_address,
                            context.nmt_state_machine.node_id,
                            current_nmt_state,
                            &context.od,
                            &context.sdo_client,
                            &context.pending_nmt_requests,
                            context.en_flag,
                        )),
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
        match serialize_and_prepare_action(context, response_frame) {
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
    match context.sdo_server.tick(current_time_us, &context.od) {
        // `tick` now returns the components directly
        Ok(Some((client_info, seq_header, command))) => {
            // SDO server generated an abort frame, needs to be sent.
            // Build the appropriate frame based on client_info.
            #[cfg(feature = "sdo-udp")]
            let build_udp = || {
                // Pass components to build function
                build_udp_from_sdo_response(context, client_info, seq_header.clone(), command.clone())
            };
            #[cfg(not(feature = "sdo-udp"))]
            let build_udp = || {
                Err::<NodeAction, PowerlinkError>(PowerlinkError::InternalError(
                    "UDP feature disabled",
                ))
            };

            match client_info {
                SdoClientInfo::Asnd { .. } => {
                    // Pass components to build function
                    match build_asnd_from_sdo_response(context, client_info, seq_header, command) {
                        Ok(action) => return action,
                        Err(e) => error!("[CN] Failed to build SDO Abort ASnd frame: {:?}", e),
                    }
                }
                #[cfg(feature = "sdo-udp")]
                SdoClientInfo::Udp { .. } => match build_udp() {
                    Ok(action) => return action,
                    Err(e) => error!("[CN] Failed to build SDO Abort UDP frame: {:?}", e),
                },
            }
            // If building the abort frame failed, fall through to other tick logic.
        }
        Err(e) => error!("[CN] SDO Server tick error: {:?}", e),
        _ => {} // No action or no error
    }

    let current_nmt_state = context.nmt_state_machine.current_state();
    // Check if a deadline is set and if it has passed
    let deadline_passed = context
        .next_tick_us
        .map_or(false, |deadline| current_time_us >= deadline);

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
        current_time_us,
        context.next_tick_us
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
                .process_event(NmtEvent::Timeout, &mut context.od);
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
                let (nmt_action, signaled) = context.dll_error_manager.handle_error(error);
                if signaled {
                    context.error_status_changed = true;
                    // Update Error Register (0x1001)
                    let current_err_reg = context.od.read_u8(OD_IDX_ERROR_REGISTER, 0).unwrap_or(0);
                    let new_err_reg = current_err_reg | 0b1; // Set Generic Error
                    context
                        .od
                        .write_internal(
                            OD_IDX_ERROR_REGISTER,
                            0,
                            crate::od::ObjectValue::Unsigned8(new_err_reg),
                            false,
                        )
                        .unwrap_or_else(|e| error!("[CN] Failed to update Error Register: {:?}", e));
                }
                if nmt_action != NmtAction::None {
                    context
                        .nmt_state_machine
                        .process_event(NmtEvent::Error, &mut context.od);
                    context.soc_timeout_check_active = false;
                    return NodeAction::NoAction; // Stop processing after NMT reset
                }
            }
        }
        // Reschedule next check if still active
        if context.soc_timeout_check_active {
            let cycle_time_opt = context.od.read_u32(OD_IDX_CYCLE_TIME, 0).map(|v| v as u64);
            let tolerance_opt = context
                .od
                .read_u32(OD_IDX_LOSS_SOC_TOLERANCE, 0)
                .map(|v| v as u64);

            if let (Some(cycle_time_us), Some(tolerance_ns)) = (cycle_time_opt, tolerance_opt) {
                if cycle_time_us > 0 {
                    let cycles_missed =
                        ((current_time_us - context.last_soc_reception_time_us) / cycle_time_us)
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

/// Helper to build ASnd frame from SdoResponseData.
fn build_asnd_from_sdo_response(
    context: &CnContext,
    client_info: SdoClientInfo,
    seq_header: SequenceLayerHeader,
    command: SdoCommand,
) -> Result<NodeAction, PowerlinkError> {
    let (source_node_id, source_mac) = match client_info {
        SdoClientInfo::Asnd {
            source_node_id,
            source_mac,
        } => (source_node_id, source_mac),
        #[cfg(feature = "sdo-udp")]
        SdoClientInfo::Udp { .. } => {
            return Err(PowerlinkError::InternalError(
                "Attempted to build ASnd response for UDP client",
            ))
        }
    };

    let sdo_payload = asnd::serialize_sdo_asnd_payload(seq_header, command)?;
    let asnd_frame = ASndFrame::new(
        context.mac_address,
        source_mac,
        source_node_id,
        context.nmt_state_machine.node_id,
        ServiceId::Sdo,
        sdo_payload,
    );
    info!(
        "Sending SDO response via ASnd to Node {}",
        source_node_id.0
    );
    serialize_and_prepare_action(context, PowerlinkFrame::ASnd(asnd_frame))
}

/// Helper to build NodeAction::SendUdp from SdoResponseData.
#[cfg(feature = "sdo-udp")]
pub(crate) fn build_udp_from_sdo_response(
    _context: &CnContext,
    client_info: SdoClientInfo,
    seq_header: SequenceLayerHeader,
    command: SdoCommand,
) -> Result<NodeAction, PowerlinkError> {
    let (source_ip, source_port) = match client_info {
        SdoClientInfo::Udp {
            source_ip,
            source_port,
        } => (source_ip, source_port),
        SdoClientInfo::Asnd { .. } => {
            return Err(PowerlinkError::InternalError(
                "Attempted to build UDP response for ASnd client",
            ))
        }
    };

    let mut udp_buffer = vec![0u8; 1500]; // MTU size
    let udp_payload_len = serialize_sdo_udp_payload(seq_header, command, &mut udp_buffer)?;
    udp_buffer.truncate(udp_payload_len);
    info!(
        "Sending SDO response via UDP to {}:{}",
        core::net::Ipv4Addr::from(source_ip),
        source_port
    );
    Ok(NodeAction::SendUdp {
        dest_ip: source_ip,
        dest_port: source_port,
        data: udp_buffer,
    })
}

/// Helper to serialize a PowerlinkFrame (Ethernet) and prepare the NodeAction.
/// Returns Result to propagate serialization errors.
fn serialize_and_prepare_action(
    _context: &CnContext,
    frame: PowerlinkFrame,
) -> Result<NodeAction, PowerlinkError> {
    // Estimate max size needed (14 Eth + Max PL size ~1500)
    let mut buf = vec![0u8; 1518];
    // Serialize Eth header first
    let eth_header = match &frame {
        PowerlinkFrame::PRes(f) => &f.eth_header,
        PowerlinkFrame::ASnd(f) => &f.eth_header,
        // Add other frame types if CN might send them (unlikely for responses)
        _ => {
            error!(
                "[CN] Attempted to serialize unexpected response frame type: {:?}",
                frame
            );
            return Ok(NodeAction::NoAction); // Return NoAction on unexpected type
        }
    };
    CodecHelpers::serialize_eth_header(eth_header, &mut buf);

    // Then serialize PL part into the buffer starting after the Eth header
    match frame.serialize(&mut buf[14..]) {
        Ok(pl_size) => {
            let total_size = 14 + pl_size;
            if total_size < 60 {
                // Ethernet minimum frame size (excluding preamble, SFD, FCS)
                // Spec requires padding, but the raw socket likely handles this.
                // We truncate to the *actual* data size.
                buf.truncate(total_size);
                trace!(
                    "Frame size {} bytes (padding likely handled by OS/hardware).",
                    total_size
                );
            } else {
                buf.truncate(total_size);
            }
            info!(
                "Sending response frame type: {:?} ({} bytes)",
                frame,
                buf.len()
            );
            trace!("Sending frame bytes ({}): {:02X?}", buf.len(), &buf);
            Ok(NodeAction::SendFrame(buf))
        }
        Err(e) => {
            error!("[CN] Failed to serialize response frame: {:?}", e);
            Err(e) // Propagate serialization error
        }
    }
}
