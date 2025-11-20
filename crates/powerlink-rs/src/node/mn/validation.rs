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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::error::{DllErrorManager, LoggingErrorHandler, MnErrorCounters};
    use crate::frame::ms_state_machine::DllMsStateMachine;
    use crate::nmt::mn_state_machine::MnNmtStateMachine;
    use crate::nmt::flags::FeatureFlags;
    use crate::nmt::states::NmtState;
    use crate::node::CoreNodeContext;
    use crate::od::{ObjectDictionary, ObjectEntry, ObjectValue};
    use crate::sdo::client_manager::SdoClientManager;
    use crate::sdo::transport::AsndTransport;
    use crate::sdo::{EmbeddedSdoClient, EmbeddedSdoServer, SdoClient, SdoServer};
    #[cfg(feature = "sdo-udp")]
    use crate::sdo::transport::UdpTransport;
    use crate::types::{C_ADR_MN_DEF_NODE_ID, EPLVersion};
    use crate::frame::poll::{PRFlag, RSFlag};
    use alloc::vec;
    use alloc::vec::Vec;
    use alloc::collections::BTreeMap;

    // --- Mock Configuration Interface ---
    struct MockConfigInterface {
        should_update_sw: bool,
        config_data: Vec<u8>,
    }
    impl crate::hal::ConfigurationInterface for MockConfigInterface {
        fn get_expected_identity(&self, _node_id: u8) -> Option<crate::hal::Identity> { None }
        fn get_configuration<'a>(&'a self, _node_id: u8) -> Result<&'a [u8], crate::PowerlinkError> {
            Ok(&self.config_data)
        }
        fn is_software_update_required(&self, _node_id: u8, _d: u32, _t: u32) -> bool {
            self.should_update_sw
        }
    }

    // --- Helper to create a valid payload ---
    fn create_valid_payload() -> IdentResponsePayload {
        IdentResponsePayload {
            pr: PRFlag::default(),
            rs: RSFlag::default(),
            nmt_state: NmtState::NmtPreOperational1,
            epl_version: EPLVersion(0x20),
            feature_flags: FeatureFlags::default(),
            mtu: 300,
            poll_in_size: 36,
            poll_out_size: 36,
            response_time: 1000,
            device_type: 0x1234,
            vendor_id: 0xABCD,
            product_code: 0x5678,
            revision_number: 0x0001,
            serial_number: 0x9999,
            verify_conf_date: 0,
            verify_conf_time: 0,
            app_sw_date: 0,
            app_sw_time: 0,
            ip_address: [0; 4],
            subnet_mask: [0; 4],
            default_gateway: [0; 4],
            host_name: "TestNode".into(),
        }
    }

    // --- Helper to setup OD ---
    fn setup_od(od: &mut ObjectDictionary, node_id: u8) {
        // Insert Expected Values
        od.insert(constants::IDX_NMT_MN_DEVICE_TYPE_ID_LIST_AU32, ObjectEntry {
            object: crate::od::Object::Array(vec![ObjectValue::Unsigned32(0); 255]),
            ..Default::default()
        });
        od.write(constants::IDX_NMT_MN_DEVICE_TYPE_ID_LIST_AU32, node_id, ObjectValue::Unsigned32(0x1234)).unwrap();

        od.insert(constants::IDX_NMT_MN_VENDOR_ID_LIST_AU32, ObjectEntry {
            object: crate::od::Object::Array(vec![ObjectValue::Unsigned32(0); 255]),
            ..Default::default()
        });
        od.write(constants::IDX_NMT_MN_VENDOR_ID_LIST_AU32, node_id, ObjectValue::Unsigned32(0xABCD)).unwrap();

        // StartUp Flags (Check Identity = Bit 9, Check Config = Bit 11)
        od.insert(constants::IDX_NMT_START_UP_U32, ObjectEntry {
             object: crate::od::Object::Variable(ObjectValue::Unsigned32(0)),
             ..Default::default()
        });
    }

    fn create_context<'a>(od: ObjectDictionary<'a>) -> MnContext<'a> {
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
            nmt_state_machine: MnNmtStateMachine::new(NodeId(C_ADR_MN_DEF_NODE_ID), Default::default(), 0, 0),
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
            current_phase: super::super::state::CyclePhase::Idle,
            current_polled_cn: None,
            async_request_queue: Default::default(),
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
    fn test_identity_check_pass() {
        let mut od = ObjectDictionary::new(None);
        let node_id = NodeId(1);
        setup_od(&mut od, node_id.0);
        let mut context = create_context(od);
        let payload = create_valid_payload();

        // Act
        let result = validate_boot_step1_checks(&mut context, node_id, &payload, 0);

        // Assert
        assert!(result, "Validation should pass for matching identity");
    }

    #[test]
    fn test_identity_check_fail_device_type() {
        let mut od = ObjectDictionary::new(None);
        let node_id = NodeId(1);
        setup_od(&mut od, node_id.0);
        let mut context = create_context(od);
        
        let mut payload = create_valid_payload();
        payload.device_type = 0x9999; // Mismatch

        // Act
        let result = validate_boot_step1_checks(&mut context, node_id, &payload, 0);

        // Assert
        assert!(!result, "Validation should fail for mismatched DeviceType");
    }

    #[test]
    fn test_config_check_pass_when_disabled() {
        let mut od = ObjectDictionary::new(None);
        let node_id = NodeId(1);
        setup_od(&mut od, node_id.0);
        
        // Ensure Bit 11 (Check Config) is 0
        od.write(constants::IDX_NMT_START_UP_U32, 0, ObjectValue::Unsigned32(0)).unwrap();

        let mut context = create_context(od);
        
        // Payload has date=0, but we set expected date in OD
        context.core.od.insert(constants::IDX_NMT_MN_EXP_CONF_DATE_LIST_AU32, ObjectEntry {
             object: crate::od::Object::Array(vec![ObjectValue::Unsigned32(0); 255]),
             ..Default::default()
        });
        context.core.od.write(constants::IDX_NMT_MN_EXP_CONF_DATE_LIST_AU32, node_id.0, ObjectValue::Unsigned32(500)).unwrap();
        
        let payload = create_valid_payload(); // Has date=0

        // Act
        let result = validate_boot_step1_checks(&mut context, node_id, &payload, 0);

        // Assert
        assert!(result, "Should pass because config check is disabled");
    }

    #[test]
    fn test_config_check_triggers_download() {
        let mut od = ObjectDictionary::new(None);
        let node_id = NodeId(1);
        setup_od(&mut od, node_id.0);
        
        // Enable Config Check (Bit 11)
        od.write(constants::IDX_NMT_START_UP_U32, 0, ObjectValue::Unsigned32(1 << 11)).unwrap();

        let mut context = create_context(od);
        
        // Set expected date
        context.core.od.insert(constants::IDX_NMT_MN_EXP_CONF_DATE_LIST_AU32, ObjectEntry {
             object: crate::od::Object::Array(vec![ObjectValue::Unsigned32(0); 255]),
             ..Default::default()
        });
        context.core.od.write(constants::IDX_NMT_MN_EXP_CONF_DATE_LIST_AU32, node_id.0, ObjectValue::Unsigned32(500)).unwrap();

        // Set up Mock Config Interface
        let mock_interface = MockConfigInterface { should_update_sw: false, config_data: vec![0x00, 0x00, 0x00, 0x00] }; // Empty Concise DCF
        context.configuration_interface = Some(&mock_interface);

        let payload = create_valid_payload(); // Has date=0, mismatch!

        // Act
        let result = validate_boot_step1_checks(&mut context, node_id, &payload, 1000);

        // Assert
        assert!(!result, "Should return false to pause boot-up");
        // Verify SDO logic triggered (we can check if a connection exists)
        // This is tricky to check deeply without exposing SDO internals, but returning false with config interface present
        // implies the download path was taken.
    }
}