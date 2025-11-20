// crates/powerlink-rs/src/node/mn/validation.rs
//! Contains logic for verifying Controlled Node (CN) identity, software, and configuration.
//! (EPSG DS 301, Section 7.4.2.2)

use super::state::{CnState, MnContext, SdoState};
use crate::frame::control::IdentResponsePayload;
use crate::od::constants;
use crate::types::NodeId;
use log::{error, info, trace, warn};

/// Validates a CN's IdentResponse payload against the MN's OD configuration.
/// (EPSG DS 301, Section 7.4.2.2.1.1, 7.4.2.2.1.2, 7.4.2.2.1.3)
///
/// This function implements active Configuration Management (CFM).
/// If a mismatch is found, and the ConfigurationInterface is present, it may
/// trigger an SDO download sequence to update the node.
///
/// Returns `true` if all checks pass and the node is ready for `BOOT_STEP2`.
/// Returns `false` if checks failed or if a remediation (download) process has been started.
pub(super) fn validate_boot_step1_checks(
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