// crates/powerlink-rs/src/node/mn/events.rs
use super::scheduler;
use super::state::{AsyncRequest, CnState, CyclePhase, MnContext};
use crate::frame::{
    ASndFrame, DllMsEvent, PResFrame, PowerlinkFrame, ServiceId,
    error::{DllError, ErrorEntry, ErrorEntryMode, NmtAction, StaticErrorBitField},
};
use crate::nmt::NmtStateMachine;
use crate::nmt::{events::NmtEvent, states::NmtState};
use crate::node::PdoHandler;
use crate::types::NodeId;
use log::{debug, error, info, trace, warn};

// --- Constants for OD access ---
const OD_IDX_STARTUP_U32: u16 = 0x1F80;
const OD_IDX_PRES_PAYLOAD_LIMIT_LIST: u16 = 0x1F8D;
const OD_IDX_DEVICE_TYPE_LIST: u16 = 0x1F84;
const OD_IDX_VENDOR_ID_LIST: u16 = 0x1F85;
const OD_IDX_PRODUCT_CODE_LIST: u16 = 0x1F86;
const OD_IDX_REVISION_NO_LIST: u16 = 0x1F87;
const OD_IDX_EXP_SW_DATE_LIST: u16 = 0x1F53;
const OD_IDX_EXP_SW_TIME_LIST: u16 = 0x1F54;
const OD_IDX_EXP_CONF_DATE_LIST: u16 = 0x1F26;
const OD_IDX_EXP_CONF_TIME_LIST: u16 = 0x1F27;

/// Internal function to process a deserialized `PowerlinkFrame`.
/// The MN primarily *consumes* PRes and ASnd frames.
/// This function is now called by `ManagingNode::process_powerlink_frame`
/// and does not handle SDO frames directly anymore.
pub(super) fn process_frame(context: &mut MnContext, frame: PowerlinkFrame, current_time_us: u64) {
    // 1. Update NMT state machine based on the frame type.
    if let Some(event) = frame.nmt_event() {
        if context.nmt_state_machine.current_state() != NmtState::NmtNotActive {
            context
                .nmt_state_machine
                .process_event(event, &mut context.core.od);
        }
    }

    // 2. Pass event to DLL state machine and handle errors.
    handle_dll_event(context, frame.dll_mn_event(), &frame);

    // 3. Handle specific frames
    match frame {
        PowerlinkFrame::PRes(pres_frame) => {
            // Update CN state based on reported NMT state
            update_cn_state(context, pres_frame.source, pres_frame.nmt_state);

            // Check if this PRes corresponds to the node we polled
            if context.current_phase == CyclePhase::IsochronousPReq
                && context.current_polled_cn == Some(pres_frame.source)
            {
                trace!(
                    "[MN] Received expected PRes from Node {}",
                    pres_frame.source.0
                );
                // Cancel pending PRes timeout
                context.pending_timeout_event = None;
                // Handle PDO consumption from PRes frames
                context.consume_pdo_payload(
                    pres_frame.source,
                    &pres_frame.payload,
                    pres_frame.pdo_version,
                    pres_frame.flags.rd,
                );
                // Handle async and error signaling flags in PRes
                handle_pres_frame(context, &pres_frame);
                // PRes received, advance to the next action in the cycle.
                // The action is returned by `tick` in the scheduler, so we don't need to capture it here.
                let _action = super::cycle::advance_cycle_phase(context, current_time_us);
            } else {
                warn!(
                    "[MN] Received unexpected PRes from Node {}.",
                    pres_frame.source.0
                );
                handle_pres_frame(context, &pres_frame);
            }
        }
        PowerlinkFrame::ASnd(asnd_frame) => {
            if context.current_phase == CyclePhase::AsynchronousSoA {
                trace!(
                    "[MN] Received ASnd from Node {} during Async phase.",
                    asnd_frame.source.0
                );
                context.pending_timeout_event = None;
                handle_asnd_frame(context, &asnd_frame);
                context.current_phase = CyclePhase::Idle;
            } else {
                handle_asnd_frame(context, &asnd_frame);
            }
        }
        _ => {
            // MN ignores SoC, PReq, and SoA frames it sent itself.
        }
    }
}

/// Passes an event to the DLL state machine and processes any resulting errors.
pub(super) fn handle_dll_event(
    context: &mut MnContext,
    event: DllMsEvent,
    frame_context: &PowerlinkFrame,
) {
    let reporting_node_id = match frame_context {
        PowerlinkFrame::PRes(f) => f.source,
        PowerlinkFrame::ASnd(f) => f.source,
        _ => context.current_polled_cn.unwrap_or(NodeId(0)),
    };
    let isochr_nodes_remaining =
        scheduler::has_more_isochronous_nodes(context, context.current_multiplex_cycle);
    let isochr = isochr_nodes_remaining || context.current_phase == CyclePhase::IsochronousPReq;

    if let Some(errors) = context.dll_state_machine.process_event(
        event,
        context.nmt_state_machine.current_state(),
        matches!(event, DllMsEvent::Pres | DllMsEvent::Asnd),
        !context.async_request_queue.is_empty(),
        !context.mn_async_send_queue.is_empty(),
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
            let (nmt_action, _) = context.dll_error_manager.handle_error(error_with_node);
            match nmt_action {
                NmtAction::ResetNode(node_id) => {
                    warn!(
                        "[MN] DLL Error threshold met for Node {}. Requesting Node Reset.",
                        node_id.0
                    );
                    if let Some(info) = context.node_info.get_mut(&node_id) {
                        info.state = CnState::Missing;
                    }
                    context
                        .pending_nmt_commands
                        .push((crate::nmt::events::NmtCommand::ResetNode, node_id));
                }
                NmtAction::ResetCommunication => {
                    warn!("[MN] DLL Error threshold met. Requesting Communication Reset.");
                    context
                        .nmt_state_machine
                        .process_event(NmtEvent::Error, &mut context.core.od);
                    context.current_phase = CyclePhase::Idle;
                }
                NmtAction::None => {}
            }
        }
    }
}

/// Handles incoming ASnd frames, such as IdentResponse or StatusResponse.
fn handle_asnd_frame(context: &mut MnContext, frame: &ASndFrame) {
    match frame.service_id {
        ServiceId::IdentResponse => {
            let node_id = frame.source;
            // Get the current state immutably, then drop the borrow
            let current_cn_state = context.node_info.get(&node_id).map(|info| info.state);

            if let Some(state) = current_cn_state {
                // Only validate if the node is currently Unknown or Missing
                if state == CnState::Unknown || state == CnState::Missing {
                    // Perform BOOT_STEP1: CHECK_IDENTIFICATION, CHECK_SOFTWARE, CHECK_CONFIGURATION
                    // This call now takes &context and does not conflict.
                    if validate_boot_step1_checks(context, node_id, &frame.payload) {
                        info!("[MN] Node {} successfully identified and validated.", node_id.0);

                        // Re-acquire mutable borrow to update state
                        if let Some(info_mut) = context.node_info.get_mut(&node_id) {
                            info_mut.state = CnState::Identified;
                        }

                        // Check if this identification allows the MN to transition
                        scheduler::check_bootup_state(context);
                    } else {
                        // Validation failed, errors already logged.
                        // Node remains in Unknown/Missing state and will be polled again.
                        info!(
                            "[MN] Node {} failed BOOT_STEP1 validation. Will remain in {:?} state.",
                            node_id.0, state
                        );
                    }
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
            if let Some(info) = context.node_info.get_mut(&node_id) {
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

/// Validates a CN's IdentResponse payload against the MN's OD configuration.
/// (EPSG DS 301, Section 7.4.2.2.1.1, 7.4.2.2.1.2, 7.4.2.2.1.3)
/// Returns true if all checks pass, false otherwise.
fn validate_boot_step1_checks(
    context: &MnContext, // <-- Takes immutable &MnContext
    node_id: NodeId,
    payload: &[u8],
) -> bool {
    // Expected payload offsets from IdentResponse (Table 135)
    const OFFSET_DEVICE_TYPE: usize = 22;
    const OFFSET_VENDOR_ID: usize = 26;
    const OFFSET_PRODUCT_CODE: usize = 30;
    const OFFSET_REVISION_NO: usize = 34;
    const OFFSET_VERIFY_CONF_DATE: usize = 50;
    const OFFSET_VERIFY_CONF_TIME: usize = 54;
    const OFFSET_APP_SW_DATE: usize = 58;
    const OFFSET_APP_SW_TIME: usize = 62;
    const MIN_PAYLOAD_LEN: usize = 66; // Need up to end of ApplicationSwTime

    if payload.len() < MIN_PAYLOAD_LEN {
        warn!(
            "[MN] IdentResponse from Node {} is too short ({} bytes) for validation.",
            node_id.0,
            payload.len()
        );
        return false;
    }

    // Helper macro to read a U32 LE from a specific offset in the payload
    macro_rules! read_payload_u32 {
        ($offset:expr) => {
            u32::from_le_bytes(
                payload[$offset..$offset + 4]
                    .try_into()
                    .unwrap_or_default(),
            )
        };
    }

    // 1. Read received values from IdentResponse payload
    let received_device_type = read_payload_u32!(OFFSET_DEVICE_TYPE);
    let received_vendor_id = read_payload_u32!(OFFSET_VENDOR_ID);
    let received_product_code = read_payload_u32!(OFFSET_PRODUCT_CODE);
    let received_revision_no = read_payload_u32!(OFFSET_REVISION_NO);
    let received_conf_date = read_payload_u32!(OFFSET_VERIFY_CONF_DATE);
    let received_conf_time = read_payload_u32!(OFFSET_VERIFY_CONF_TIME);
    let received_sw_date = read_payload_u32!(OFFSET_APP_SW_DATE);
    let received_sw_time = read_payload_u32!(OFFSET_APP_SW_TIME);

    // 2. Read expected values from MN's Object Dictionary
    // A value of 0 in the OD means "do not check"
    let expected_device_type = context
        .core
        .od
        .read_u32(OD_IDX_DEVICE_TYPE_LIST, node_id.0)
        .unwrap_or(0);
    let expected_vendor_id = context
        .core
        .od
        .read_u32(OD_IDX_VENDOR_ID_LIST, node_id.0)
        .unwrap_or(0);
    let expected_product_code = context
        .core
        .od
        .read_u32(OD_IDX_PRODUCT_CODE_LIST, node_id.0)
        .unwrap_or(0);
    let expected_revision_no = context
        .core
        .od
        .read_u32(OD_IDX_REVISION_NO_LIST, node_id.0)
        .unwrap_or(0);
    let expected_sw_date = context
        .core
        .od
        .read_u32(OD_IDX_EXP_SW_DATE_LIST, node_id.0)
        .unwrap_or(0);
    let expected_sw_time = context
        .core
        .od
        .read_u32(OD_IDX_EXP_SW_TIME_LIST, node_id.0)
        .unwrap_or(0);
    let expected_conf_date = context
        .core
        .od
        .read_u32(OD_IDX_EXP_CONF_DATE_LIST, node_id.0)
        .unwrap_or(0);
    let expected_conf_time = context
        .core
        .od
        .read_u32(OD_IDX_EXP_CONF_TIME_LIST, node_id.0)
        .unwrap_or(0);
    let startup_flags = context.core.od.read_u32(OD_IDX_STARTUP_U32, 0).unwrap_or(0);

    // 3. Perform validation checks
    // --- CHECK_IDENTIFICATION (7.4.2.2.1.1) ---
    if expected_device_type != 0 && received_device_type != expected_device_type {
        error!(
            "[MN] CHECK_IDENTIFICATION failed for Node {}: DeviceType mismatch. Expected {:#010X}, got {:#010X}. [E_NMT_BPO1_DEVICE_TYPE]",
            node_id.0, expected_device_type, received_device_type
        );
        return false;
    }
    if expected_vendor_id != 0 && received_vendor_id != expected_vendor_id {
        error!(
            "[MN] CHECK_IDENTIFICATION failed for Node {}: VendorId mismatch. Expected {:#010X}, got {:#010X}. [E_NMT_BPO1_VENDOR_ID]",
            node_id.0, expected_vendor_id, received_vendor_id
        );
        return false;
    }
    if expected_product_code != 0 && received_product_code != expected_product_code {
        error!(
            "[MN] CHECK_IDENTIFICATION failed for Node {}: ProductCode mismatch. Expected {:#010X}, got {:#010X}. [E_NMT_BPO1_PRODUCT_CODE]",
            node_id.0, expected_product_code, received_product_code
        );
        return false;
    }
    if expected_revision_no != 0 && received_revision_no != expected_revision_no {
        error!(
            "[MN] CHECK_IDENTIFICATION failed for Node {}: RevisionNo mismatch. Expected {:#010X}, got {:#010X}. [E_NMT_BPO1_REVISION_NO]",
            node_id.0, expected_revision_no, received_revision_no
        );
        return false;
    }

    trace!(
        "[MN] CHECK_IDENTIFICATION passed for Node {}.",
        node_id.0
    );

    // --- CHECK_SOFTWARE (7.4.2.2.1.2) ---
    // Check NMT_StartUp_U32.Bit10 (Software Version Check)
    if (startup_flags & (1 << 10)) != 0 {
        if expected_sw_date == 0 && expected_sw_time == 0 {
            // MN does not have an expected version configured
            warn!(
                "[MN] CHECK_SOFTWARE failed for Node {}: MN validation is enabled (0x1F80.10) but no expected SW version is configured (0x1F53/0x1F54). [E_NMT_BPO1_SW_INVALID]",
                node_id.0
            );
            return false;
        }

        if received_sw_date != expected_sw_date || received_sw_time != expected_sw_time {
            error!(
                "[MN] CHECK_SOFTWARE failed for Node {}: SW version mismatch. Expected {}/{}, got {}/{}. [E_NMT_BPO1_SW_INVALID]",
                node_id.0, expected_sw_date, expected_sw_time, received_sw_date, received_sw_time
            );
            // TODO: In the future, trigger software update logic here.
            // For now, we fail the boot-up step.
            return false;
        }

        trace!("[MN] CHECK_SOFTWARE passed for Node {}.", node_id.0);
    } else {
        trace!("[MN] CHECK_SOFTWARE skipped for Node {}.", node_id.0);
    }

    // --- CHECK_CONFIGURATION (7.4.2.2.1.3) ---
    // Check NMT_StartUp_U32.Bit11 (Configuration Check)
    if (startup_flags & (1 << 11)) != 0 {
        if expected_conf_date == 0 && expected_conf_time == 0 {
            // MN does not have an expected configuration configured
            warn!(
                "[MN] CHECK_CONFIGURATION failed for Node {}: MN validation is enabled (0x1F80.11) but no expected config date/time is configured (0x1F26/0x1F27). [E_NMT_BPO1_CF_VERIFY]",
                node_id.0
            );
            return false;
        }

        if received_conf_date != expected_conf_date || received_conf_time != expected_conf_time {
            error!(
                "[MN] CHECK_CONFIGURATION failed for Node {}: Config date/time mismatch. Expected {}/{}, got {}/{}. [E_NMT_BPO1_CF_VERIFY]",
                node_id.0, expected_conf_date, expected_conf_time, received_conf_date, received_conf_time
            );
            // TODO: In the future, trigger configuration download (SDO) logic here.
            // For now, we fail the boot-up step.
            return false;
        }

        trace!("[MN] CHECK_CONFIGURATION passed for Node {}.", node_id.0);
    } else {
        trace!("[MN] CHECK_CONFIGURATION skipped for Node {}.", node_id.0);
    }

    // TODO: Add SerialNo check (0x1F88) as a warning-only check

    true
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
fn handle_pres_frame(context: &mut MnContext, pres: &PResFrame) {
    // 1. Handle async requests flagged by RS.
    if pres.flags.rs.get() > 0 {
        debug!("[MN] Node {} requesting async transmission.", pres.source.0);
        context.async_request_queue.push(AsyncRequest {
            node_id: pres.source,
            priority: pres.flags.pr as u8,
        });
    }

    // 2. Handle error signaling with EN/EA flags.
    if let Some(info) = context.node_info.get_mut(&pres.source) {
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
            if !context.pending_status_requests.contains(&pres.source) {
                context.pending_status_requests.push(pres.source);
            }
        }

        // --- Phase 1.5: CHECK_COMMUNICATION ---
        // This check is performed when the MN is in ReadyToOperate, before moving to Operational.
        if context.nmt_state_machine.current_state() == NmtState::NmtReadyToOperate
            && !info.communication_ok
        {
            let expected_payload_size = context
                .core
                .od
                .read_u16(OD_IDX_PRES_PAYLOAD_LIMIT_LIST, pres.source.0)
                .unwrap_or(0) as usize;

            // Check payload size. We already checked for timeouts when the PRes was received.
            // Spec 7.4.2.2.3: check payload length is "less or equal than the length configured"
            if pres.payload.len() <= expected_payload_size {
                trace!(
                    "[MN] CHECK_COMMUNICATION passed for Node {}. (Payload size {} <= {}).",
                    pres.source.0,
                    pres.payload.len(),
                    expected_payload_size
                );
                info.communication_ok = true;
                // Immediately check if this was the last node needed for NMT transition
                scheduler::check_bootup_state(context);
            } else {
                error!(
                    "[MN] CHECK_COMMUNICATION failed for Node {}: PRes payload size mismatch. Expected <= {}, got {}.",
                    pres.source.0,
                    expected_payload_size,
                    pres.payload.len()
                );
                // TODO: Handle error (e.g., E_NMT_BRO)
            }
        }
    }
}

/// Updates the MN's internal state tracker for a CN based on its reported NMT state.
fn update_cn_state(context: &mut MnContext, node_id: NodeId, reported_state: NmtState) {
    if let Some(current_info) = context.node_info.get_mut(&node_id) {
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

            // If a node resets (e.g., error) or stops, its communication
            // must be re-verified when it comes back online.
            if new_state < CnState::PreOperational {
                current_info.communication_ok = false;
            }
            scheduler::check_bootup_state(context);
        }
    }
}