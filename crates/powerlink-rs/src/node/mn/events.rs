// crates/powerlink-rs/src/node/mn/events.rs
use super::scheduler;
use super::state::{AsyncRequest, CnState, CyclePhase, MnContext, SdoState};
use crate::frame::{
    ASndFrame, DllMsEvent, PResFrame, PowerlinkFrame, ServiceId,
    control::{IdentResponsePayload, StatusResponsePayload},
    error::{DllError, NmtAction},
};
use crate::nmt::NmtStateMachine;
use crate::nmt::events::{MnNmtCommandRequest, NmtStateCommand};
use crate::nmt::{events::NmtEvent, states::NmtState};
use crate::node::PdoHandler;
use crate::node::mn::ip_from_node_id;
use crate::node::mn::state::NmtCommandData;
use crate::od::constants;
use crate::types::NodeId;
use log::{debug, error, info, trace, warn};

/// Processes a `PowerlinkFrame` after it has been identified as
/// non-SDO or not for the MN. This handles NMT state changes and
/// DLL state progression based on received frames.
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
            // --- Increment Diagnostic Counters ---
            context.core.od.increment_counter(
                constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
                constants::SUBIDX_DIAG_NMT_COUNT_ISOCHR_RX,
            );
            // --- End of Diagnostic Counters ---

            trace!("[MN] Received PRes from Node {}", pres_frame.source.0);
            if let Some(cn_info) = context.node_info.get_mut(&pres_frame.source) {
                cn_info.nmt_state = pres_frame.nmt_state;
                cn_info.last_pres_time_us = current_time_us;
                cn_info.dll_errors = 0; // Clear error count on successful PRes
                // Update the CN's state in the MN's tracker
                update_cn_state(context, pres_frame.source, pres_frame.nmt_state);
            }

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
            // --- Increment Diagnostic Counters ---
            // SDO ASnd frames are handled in `main.rs`. This only counts non-SDO ASnd.
            context.core.od.increment_counter(
                constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
                constants::SUBIDX_DIAG_NMT_COUNT_ASYNC_RX,
            );
            // --- End of Diagnostic Counters ---

            if context.current_phase == CyclePhase::AsynchronousSoA {
                trace!(
                    "[MN] Received ASnd from Node {} during Async phase.",
                    asnd_frame.source.0
                );
                context.pending_timeout_event = None;
                handle_asnd_frame(context, &asnd_frame, current_time_us); // Pass time
                context.current_phase = CyclePhase::Idle;
            } else {
                handle_asnd_frame(context, &asnd_frame, current_time_us); // Pass time
            }
        }
        _ => {
            // Other frames (SoC, PReq, SoA) are sent by the MN, not received.
            // SDO ASnd is handled in main.rs
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
                    context.pending_nmt_commands.push((
                        MnNmtCommandRequest::State(NmtStateCommand::ResetNode),
                        node_id,
                        NmtCommandData::None,
                    ));
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
fn handle_asnd_frame(context: &mut MnContext, frame: &ASndFrame, current_time_us: u64) {
    match frame.service_id {
        ServiceId::IdentResponse => {
            let node_id = frame.source;
            // Get the current state immutably, then drop the borrow
            let current_cn_state = context.node_info.get(&node_id).map(|info| info.state);

            if let Some(state) = current_cn_state {
                // Only validate if the node is currently Unknown or Missing
                if state == CnState::Unknown || state == CnState::Missing {
                    match IdentResponsePayload::deserialize(&frame.payload) {
                        Ok(payload) => {
                            // Perform Boot Step 1 Checks (ID, SW, Config)
                            // This function may trigger SDO downloads (remediation).
                            if validate_boot_step1_checks(
                                context,
                                node_id,
                                &payload,
                                current_time_us,
                            ) {
                                info!(
                                    "[MN] Node {} successfully identified and validated.",
                                    node_id.0
                                );

                                // --- ARP CACHE LOGIC ---
                                // Passively populate the ARP cache based on the frame
                                // (Spec 5.1.3)
                                let cn_ip = ip_from_node_id(node_id);
                                let cn_mac = frame.eth_header.source_mac;
                                context.arp_cache.insert(cn_ip, cn_mac);
                                info!(
                                    "[MN-ARP] Cached MAC {} for Node {} (IP {}).",
                                    cn_mac,
                                    node_id.0,
                                    core::net::Ipv4Addr::from(cn_ip)
                                );

                                // Re-acquire mutable borrow to update state
                                if let Some(info_mut) = context.node_info.get_mut(&node_id) {
                                    info_mut.state = CnState::Identified;
                                    // Cache the identity info
                                    info_mut.identity = Some(super::state::CnIdentity {
                                        device_type: payload.device_type,
                                        vendor_id: payload.vendor_id,
                                        product_code: payload.product_code,
                                        revision_no: payload.revision_number,
                                        serial_no: payload.serial_number,
                                    });
                                }
                                // Check if this identification allows the MN to transition
                                scheduler::check_bootup_state(context);
                            } else {
                                // Validation failed, OR remediation (download) started.
                                // We stay in Unknown/Missing effectively, waiting for the next IdentResponse
                                // (after reset) or for the SDO download to complete.
                                info!(
                                    "[MN] Node {} failed BOOT_STEP1 validation or is updating. Remaining in {:?} state.",
                                    node_id.0, state
                                );
                            }
                        }
                        Err(e) => {
                            error!(
                                "[MN] Failed to deserialize IdentResponse from Node {}: {:?}",
                                node_id.0, e
                            );
                        }
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

                match StatusResponsePayload::deserialize(&frame.payload) {
                    Ok(payload) => {
                        info!(
                            "[MN] StatusResponse from Node {}: ErrorRegister = {:#04x}, SpecificErrors = {:02X?}",
                            node_id.0,
                            payload.static_error_bit_field.error_register,
                            payload.static_error_bit_field.specific_errors
                        );
                        // Update the CN's state in the MN's tracker
                        update_cn_state(context, node_id, payload.nmt_state);

                        for entry in payload.error_entries {
                            warn!(
                                "[MN] StatusResponse from Node {}: Received Error/Event Entry: {:?}",
                                node_id.0, entry
                            );
                        }
                    }
                    Err(e) => {
                        error!(
                            "[MN] Failed to deserialize StatusResponse from Node {}: {:?}",
                            node_id.0, e
                        );
                    }
                }
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
///
/// This function implements active Configuration Management (CFM).
/// If a mismatch is found, and the ConfigurationInterface is present, it may
/// trigger an SDO download sequence to update the node.
///
/// Returns `true` if all checks pass and the node is ready for `BOOT_STEP2`.
/// Returns `false` if checks failed or if a remediation (download) process has been started.
fn validate_boot_step1_checks(
    context: &mut MnContext,
    node_id: NodeId,
    payload: &IdentResponsePayload,
    current_time_us: u64,
) -> bool {
    // 1. Read received values from IdentResponse payload (already deserialized)
    let received_device_type = payload.device_type;
    let received_vendor_id = payload.vendor_id;
    let received_product_code = payload.product_code;
    let received_revision_no = payload.revision_number;
    let received_conf_date = payload.verify_conf_date;
    let received_conf_time = payload.verify_conf_time;
    let received_sw_date = payload.app_sw_date;
    let received_sw_time = payload.app_sw_time;

    // 2. Read expected values from MN's Object Dictionary
    let expected_device_type = context
        .core
        .od
        .read_u32(constants::IDX_NMT_MN_DEVICE_TYPE_ID_LIST_AU32, node_id.0)
        .unwrap_or(0);
    let expected_vendor_id = context
        .core
        .od
        .read_u32(constants::IDX_NMT_MN_VENDOR_ID_LIST_AU32, node_id.0)
        .unwrap_or(0);
    let expected_product_code = context
        .core
        .od
        .read_u32(constants::IDX_NMT_MN_PRODUCT_CODE_LIST_AU32, node_id.0)
        .unwrap_or(0);
    let expected_revision_no = context
        .core
        .od
        .read_u32(constants::IDX_NMT_MN_REVISION_NO_LIST_AU32, node_id.0)
        .unwrap_or(0);
    let expected_sw_date = context
        .core
        .od
        .read_u32(constants::IDX_NMT_MN_EXP_APP_SW_DATE_LIST_AU32, node_id.0)
        .unwrap_or(0);
    let expected_sw_time = context
        .core
        .od
        .read_u32(constants::IDX_NMT_MN_EXP_APP_SW_TIME_LIST_AU32, node_id.0)
        .unwrap_or(0);
    let expected_conf_date = context
        .core
        .od
        .read_u32(constants::IDX_NMT_MN_EXP_CONF_DATE_LIST_AU32, node_id.0)
        .unwrap_or(0);
    let expected_conf_time = context
        .core
        .od
        .read_u32(constants::IDX_NMT_MN_EXP_CONF_TIME_LIST_AU32, node_id.0)
        .unwrap_or(0);
    let startup_flags = context
        .core
        .od
        .read_u32(constants::IDX_NMT_START_UP_U32, 0)
        .unwrap_or(0);

    // 3. Perform validation checks
    // --- CHECK_IDENTIFICATION (7.4.2.2.1.1) ---
    // (Identity mismatch usually requires a physical device replacement, not just config download)
    if expected_device_type != 0 && received_device_type != expected_device_type {
        error!(
            "[MN] CHECK_IDENTIFICATION failed Node {}: DeviceType mismatch.",
            node_id.0
        );
        return false;
    }
    if expected_vendor_id != 0 && received_vendor_id != expected_vendor_id {
        error!(
            "[MN] CHECK_IDENTIFICATION failed Node {}: VendorId mismatch.",
            node_id.0
        );
        return false;
    }
    if expected_product_code != 0 && received_product_code != expected_product_code {
        error!(
            "[MN] CHECK_IDENTIFICATION failed Node {}: ProductCode mismatch.",
            node_id.0
        );
        return false;
    }
    if expected_revision_no != 0 && received_revision_no != expected_revision_no {
        error!(
            "[MN] CHECK_IDENTIFICATION failed Node {}: RevisionNo mismatch.",
            node_id.0
        );
        return false;
    }

    trace!("[MN] CHECK_IDENTIFICATION passed for Node {}.", node_id.0);

    // --- CHECK_SOFTWARE (7.4.2.2.1.2) ---
    if (startup_flags & (1 << 10)) != 0 {
        // Check if software update is required via the HAL
        // The HAL might use the received dates or internal logic
        let sw_update_required = if let Some(cfg_if) = context.configuration_interface {
            cfg_if.is_software_update_required(node_id.0, received_sw_date, received_sw_time)
        } else {
            // Fallback to simple OD comparison if no HAL interface
            expected_sw_date != 0
                && (received_sw_date != expected_sw_date || received_sw_time != expected_sw_time)
        };

        if sw_update_required {
            warn!(
                "[MN] CHECK_SOFTWARE failed for Node {}. Update required.",
                node_id.0
            );
            // TODO: Trigger Program Download (PDL) here if supported.
            // For now, we just fail, as PDL is a separate complex process.
            return false;
        }
        trace!("[MN] CHECK_SOFTWARE passed for Node {}.", node_id.0);
    }

    // --- CHECK_CONFIGURATION (7.4.2.2.1.3) ---
    if (startup_flags & (1 << 11)) != 0 {
        let config_mismatch = expected_conf_date != 0
            && (received_conf_date != expected_conf_date
                || received_conf_time != expected_conf_time);

        if config_mismatch {
            warn!(
                "[MN] CHECK_CONFIGURATION failed for Node {}. Expected {}/{}, Got {}/{}.",
                node_id.0,
                expected_conf_date,
                expected_conf_time,
                received_conf_date,
                received_conf_time
            );

            // --- REMEDIATION LOGIC ---
            // If we have a configuration interface, try to fetch the configuration and start download.
            if let Some(cfg_if) = context.configuration_interface {
                info!(
                    "[MN-CFM] Attempting to retrieve configuration for Node {} from application.",
                    node_id.0
                );
                match cfg_if.get_configuration(node_id.0) {
                    Ok(concise_dcf) => {
                        info!(
                            "[MN-CFM] Starting SDO Configuration Download ({} bytes) for Node {}.",
                            concise_dcf.len(),
                            node_id.0
                        );

                        // Trigger the SdoClientManager to start the sequence
                        if let Err(e) = context.sdo_client_manager.start_configuration_download(
                            node_id,
                            concise_dcf.to_vec(),
                            current_time_us,
                            &context.core.od,
                        ) {
                            error!("[MN-CFM] Failed to start configuration download: {:?}", e);
                        } else {
                            // Update internal state to indicate SDO is in progress
                            if let Some(info) = context.node_info.get_mut(&node_id) {
                                info.sdo_state = SdoState::InProgress;
                            }
                        }
                        // Return false because the node is NOT ready yet.
                        // It will be ready after SDO finishes and it (likely) resets.
                        return false;
                    }
                    Err(e) => {
                        error!(
                            "[MN-CFM] Application failed to provide configuration for Node {}: {:?}",
                            node_id.0, e
                        );
                        return false;
                    }
                }
            } else {
                error!(
                    "[MN] Configuration mismatch, but no Configuration Interface provided to fix it."
                );
                return false;
            }
        }
        trace!("[MN] CHECK_CONFIGURATION passed for Node {}.", node_id.0);
    }

    // TODO: Add SerialNo check (0x1F88) as a warning-only check
    true
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
                .read_u16(constants::IDX_NMT_PRES_PAYLOAD_LIMIT_AU16, pres.source.0)
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
