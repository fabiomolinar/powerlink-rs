use super::events;
use super::state::CnContext;
use crate::frame::basic::MacAddress;
use crate::frame::error::{
    CnErrorCounters, DllErrorManager, ErrorCounters, ErrorHandler, LoggingErrorHandler,
};
use crate::frame::{deserialize_frame, DllError, NmtAction, PReqFrame, PResFrame};
use crate::nmt::cn_state_machine::CnNmtStateMachine;
use crate::nmt::events::{NmtCommand, NmtEvent};
use crate::nmt::state_machine::NmtStateMachine;
use crate::nmt::states::NmtState;
use crate::node::{CoreNodeContext, Node, NodeAction, PdoHandler}; // Import CoreNodeContext
use crate::od::ObjectDictionary;
#[cfg(feature = "sdo-udp")]
use crate::sdo::server::SdoClientInfo;
use crate::sdo::{SdoServer, SdoClient};
use crate::types::{NodeId, C_ADR_MN_DEF_NODE_ID};
use crate::PowerlinkError;
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use log::{error, info, trace, warn, debug};

const OD_IDX_ERROR_REGISTER: u16 = 0x1001;

/// Represents a complete POWERLINK Controlled Node (CN).
/// This struct is a thin wrapper around a context object that holds all state.
pub struct ControlledNode<'s> {
    context: CnContext<'s>,
}

impl<'s> ControlledNode<'s> {
    /// Creates a new Controlled Node.
    ///
    /// The application is responsible for creating and populating the Object Dictionary
    /// with device-specific parameters (e.g., Identity Object 0x1018) before passing
    /// it to this constructor. This function will then read the necessary configuration
    /// from the OD to initialize the NMT state machine.
    pub fn new(
        mut od: ObjectDictionary<'s>,
        mac_address: MacAddress,
    ) -> Result<Self, PowerlinkError> {
        info!("Creating new Controlled Node.");
        // Initialise the OD, which involves loading from storage or applying defaults.
        od.init()?;

        // Validate that the user-provided OD contains all mandatory objects.
        od.validate_mandatory_objects(false)?; // false for CN validation

        // The NMT state machine's constructor is now fallible because it must
        // read critical parameters from the fully configured OD.
        let nmt_state_machine = CnNmtStateMachine::from_od(&od)?;

        // --- Instantiate CoreNodeContext ---
        let core_context = CoreNodeContext {
            od,
            mac_address,
            sdo_server: SdoServer::new(),
            sdo_client: SdoClient::new(),
        };

        let mut node = Self {
            context: CnContext {
                core: core_context, // Use the new core context
                nmt_state_machine,
                dll_state_machine: Default::default(),
                dll_error_manager: DllErrorManager::new(CnErrorCounters::new(), LoggingErrorHandler),
                pending_nmt_requests: Vec::new(),
                emergency_queue: VecDeque::with_capacity(10), // Default capacity for 10 errors
                last_soc_reception_time_us: 0,
                soc_timeout_check_active: false,
                next_tick_us: None,
                en_flag: false,
                // Per spec 6.5.5.1, EC starts as 1 to indicate "not initialized"
                ec_flag: true,
                error_status_changed: false,
            },
        };

        // Run the initial state transitions to get to NmtNotActive.
        node.context
            .nmt_state_machine
            .run_internal_initialisation(&mut node.context.core.od); // Access OD through core

        Ok(node)
    }

    /// Allows the application to queue an SDO request payload to be sent.
    pub fn queue_sdo_request(&mut self, payload: Vec<u8>) {
        self.context.core.queue_sdo_request(NodeId(C_ADR_MN_DEF_NODE_ID), payload);
    }

    /// Allows the application to queue an NMT command request to be sent to the MN.
    /// (Reference: EPSG DS 301, Section 7.3.6)
    pub fn queue_nmt_request(&mut self, command: NmtCommand, target: NodeId) {
        info!(
            "Queueing NMT request: Command={:?}, Target={}",
            command, target.0
        );
        self.context.pending_nmt_requests.push((command, target));
    }

    /// Processes a POWERLINK Ethernet frame.
    fn process_ethernet_frame(
        &mut self,
        buffer: &[u8],
        current_time_us: u64,
    ) -> NodeAction {
        match deserialize_frame(buffer) {
            Ok(frame) => events::process_frame(&mut self.context, frame, current_time_us),
            Err(e) if e != PowerlinkError::InvalidEthernetFrame => {
                // Looked like POWERLINK (correct EtherType) but malformed. Log as warning.
                warn!(
                    "[CN] Could not deserialize potential POWERLINK frame: {:?} (Buffer len: {})",
                    e,
                    buffer.len()
                );
                // Report as InvalidFormat DLL error
                let (nmt_action, signaled) = self
                    .context
                    .dll_error_manager
                    .handle_error(DllError::InvalidFormat);
                if signaled {
                    self.context.error_status_changed = true;
                    // Update Error Register (0x1001), Set Bit 0: Generic Error
                    let current_err_reg =
                        self.context.core.od.read_u8(OD_IDX_ERROR_REGISTER, 0).unwrap_or(0);
                    let new_err_reg = current_err_reg | 0b1;
                    self.context
                        .core
                        .od
                        .write_internal(
                            OD_IDX_ERROR_REGISTER,
                            0,
                            crate::od::ObjectValue::Unsigned8(new_err_reg),
                            false,
                        )
                        .unwrap_or_else(|e| error!("[CN] Failed to update Error Register: {:?}", e));
                }
                // Trigger NMT error handling if required
                if nmt_action != NmtAction::None {
                    self.context
                        .nmt_state_machine
                        .process_event(NmtEvent::Error, &mut self.context.core.od);
                }
                NodeAction::NoAction
            }
            _ => NodeAction::NoAction, // Ignore other EtherTypes silently
        }
    }

    /// Consumes the payload of a PReq frame based on RPDO mapping.
    fn consume_preq_payload(&mut self, preq: &PReqFrame) {
        // Node ID 0 is reserved for PReq source according to spec and OD usage
        self.context.consume_pdo_payload(
            NodeId(0),
            &preq.payload,
            preq.pdo_version,
            preq.flags.rd, // Pass the RD flag
        );
    }

    /// Consumes the payload of a PRes frame based on RPDO mapping.
    fn consume_pres_payload(&mut self, pres: &PResFrame) {
        self.context.consume_pdo_payload(
            pres.source, // Source Node ID of the PRes
            &pres.payload,
            pres.pdo_version,
            pres.flags.rd, // Pass the RD flag
        );
    }
}

impl<'s> Node for ControlledNode<'s> {
    /// Processes a raw byte buffer received from the network at a specific time.
    /// This now tries to interpret the buffer as either Ethernet or UDP (if enabled).
    fn process_raw_frame(&mut self, buffer: &[u8], current_time_us: u64) -> NodeAction {
        // --- Try Ethernet Frame Processing ---
        // Check length and EtherType
        if buffer.len() >= 14 && buffer[12..14] == crate::types::C_DLL_ETHERTYPE_EPL.to_be_bytes()
        {
            // Check if we are in BasicEthernet
            if self.nmt_state() == NmtState::NmtBasicEthernet {
                info!(
                    "[CN] POWERLINK frame detected in NmtBasicEthernet. Transitioning to NmtPreOperational1."
                );
                // Trigger the NMT transition
                self.context
                    .nmt_state_machine
                    .process_event(NmtEvent::PowerlinkFrameReceived, &mut self.context.core.od);
                // Fall through to process the frame that triggered the transition
            }
            return self.process_ethernet_frame(buffer, current_time_us);
        }

        #[cfg(feature = "sdo-udp")]
        {
            trace!("Ignoring non-POWERLINK Ethernet frame (potential UDP?).");
        }

        trace!("Ignoring unknown frame type or non-PL Ethernet frame.");
        NodeAction::NoAction
    }

    fn tick(&mut self, current_time_us: u64) -> NodeAction {
        events::process_tick(&mut self.context, current_time_us)
    }

    fn nmt_state(&self) -> NmtState {
        self.context.nmt_state_machine.current_state()
    }

    fn next_action_time(&self) -> Option<u64> {
        self.context.next_tick_us
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        nmt::flags::FeatureFlags,
        od::{AccessType, Category, Object, ObjectEntry, ObjectValue, PdoMapping},
    };
    use alloc::vec;

    // Helper function to create a minimal Object Dictionary for CN tests.
    fn get_test_od() -> ObjectDictionary<'static> {
        let mut od = ObjectDictionary::new(None);
        let node_id = 42u8;

        // --- Common Mandatory Objects ---
        od.insert(
            0x1000, // NMT_DeviceType_U32
            ObjectEntry {
                object: Object::Variable(ObjectValue::Unsigned32(0x000F0191)), // Example value
                name: "DeviceType",
                category: Category::Mandatory,
                access: Some(AccessType::Constant),
                default_value: None,
                value_range: None,
                pdo_mapping: None,
            },
        );
        od.insert(
            0x1018, // NMT_IdentityObject_REC
            ObjectEntry {
                object: Object::Record(vec![
                    ObjectValue::Unsigned32(1), // VendorId
                    ObjectValue::Unsigned32(2), // ProductCode
                    ObjectValue::Unsigned32(3), // RevisionNo
                    ObjectValue::Unsigned32(4), // SerialNo
                ]),
                name: "IdentityObject",
                category: Category::Mandatory,
                access: None,
                default_value: None,
                value_range: None,
                pdo_mapping: None,
            },
        );
        let flags = FeatureFlags::ISOCHRONOUS | FeatureFlags::SDO_ASND;
        od.insert(
            0x1F82, // NMT_FeatureFlags_U32
            ObjectEntry {
                object: Object::Variable(ObjectValue::Unsigned32(flags.0)),
                name: "FeatureFlags",
                category: Category::Mandatory,
                access: Some(AccessType::Constant),
                default_value: None,
                value_range: None,
                pdo_mapping: None,
            },
        );

        // --- CN Specific Mandatory Objects ---
        od.insert(
            0x1F93, // NMT_EPLNodeID_REC
            ObjectEntry {
                object: Object::Record(vec![
                    ObjectValue::Unsigned8(node_id),
                    ObjectValue::Boolean(0),
                ]),
                name: "NodeIDConfig",
                category: Category::Mandatory,
                access: None,
                default_value: None,
                value_range: None,
                pdo_mapping: None,
            },
        );
        od.insert(
            0x1F99, // NMT_CNBasicEthernetTimeout_U32
            ObjectEntry {
                object: Object::Variable(ObjectValue::Unsigned32(5_000_000)), // 5 seconds
                name: "BasicEthTimeout",
                category: Category::Mandatory,
                access: Some(AccessType::ReadWriteStore),
                default_value: None,
                value_range: None,
                pdo_mapping: None,
            },
        );

        // --- Other Objects Needed by Tests/Code ---
        od.insert(
            0x1F8C, // NMT_CurrNMTState_U8 (Used by update_od_state)
            ObjectEntry {
                object: Object::Variable(ObjectValue::Unsigned8(0)),
                name: "CurrentNMTState",
                category: Category::Mandatory, // Spec lists as Mandatory
                access: Some(AccessType::ReadOnly),
                default_value: None,
                value_range: None,
                pdo_mapping: Some(PdoMapping::No), // Spec lists mapping as No
            },
        );
        od.insert(
            0x1006, // NMT_CycleLen_U32 (Needed for SoC timeout scheduling)
            ObjectEntry {
                object: Object::Variable(ObjectValue::Unsigned32(10000)), // 10ms example
                name: "CycleLength",
                category: Category::Mandatory, // Spec lists as Mandatory
                access: Some(AccessType::ReadWriteStore),
                default_value: None,
                value_range: None,
                pdo_mapping: None,
            },
        );
        od.insert(
            0x1C14, // DLL_CNLossOfSocTolerance_U32 (Needed for SoC timeout scheduling)
            ObjectEntry {
                object: Object::Variable(ObjectValue::Unsigned32(100000)), // 100us example
                name: "LossSocTolerance",
                category: Category::Mandatory, // Spec lists as Mandatory for CN
                access: Some(AccessType::ReadWriteStore),
                default_value: None,
                value_range: None,
                pdo_mapping: None,
            },
        );
        // Add minimal PDO config objects required by payload::build_pres_response
        od.insert(
            0x1800, // TPDO Comm Param (for PRes)
            ObjectEntry {
                object: Object::Record(vec![
                    ObjectValue::Unsigned8(node_id),
                    ObjectValue::Unsigned8(0),
                ]),
                name: "TPDO1CommParam",
                category: Category::Mandatory,
                access: None,
                default_value: None,
                value_range: None,
                pdo_mapping: None,
            },
        );
        od.insert(
            0x1A00, // TPDO Mapping Param (for PRes)
            ObjectEntry {
                object: Object::Array(vec![]), // Empty mapping
                name: "TPDO1MapParam",
                category: Category::Mandatory,
                access: None,
                default_value: None,
                value_range: None,
                pdo_mapping: None,
            },
        );
        od.insert(
            0x1F98, // NMT_CycleTiming_REC (needed for PresActPayloadLimit)
            ObjectEntry {
                object: Object::Record(vec![
                    ObjectValue::Unsigned16(1490),
                    ObjectValue::Unsigned16(1490),
                    ObjectValue::Unsigned32(10000),
                    ObjectValue::Unsigned16(100),
                    ObjectValue::Unsigned16(36), // PresActPayloadLimit_U16 = 36
                    ObjectValue::Unsigned32(20000),
                    ObjectValue::Unsigned8(0),
                    ObjectValue::Unsigned16(300),
                    ObjectValue::Unsigned16(2),
                ]),
                name: "CycleTiming",
                category: Category::Mandatory,
                access: None,
                default_value: None,
                value_range: None,
                pdo_mapping: None,
            },
        );
        od.insert(
            0x1001, // ERR_ErrorRegister_U8 (used in build_status_response)
            ObjectEntry {
                object: Object::Variable(ObjectValue::Unsigned8(0)),
                name: "ErrorRegister",
                category: Category::Mandatory,
                access: Some(AccessType::ReadOnly),
                default_value: None,
                value_range: None,
                pdo_mapping: Some(PdoMapping::Optional),
            },
        );

        od
    }

    // Helper for creating a CN state machine for tests
    fn get_test_nmt() -> CnNmtStateMachine {
        let node_id = NodeId::try_from(42).unwrap();
        let feature_flags = FeatureFlags::ISOCHRONOUS | FeatureFlags::SDO_ASND;
        CnNmtStateMachine::new(node_id, feature_flags, 5_000_000)
    }

    #[test]
    fn test_from_od_reads_parameters() {
        let od = get_test_od();
        let nmt = CnNmtStateMachine::from_od(&od).unwrap();
        assert_eq!(nmt.node_id, NodeId(42));
        assert!(nmt.feature_flags.contains(FeatureFlags::SDO_ASND));
        assert_eq!(nmt.basic_ethernet_timeout, 5_000_000);
    }

    #[test]
    fn test_from_od_fails_if_missing_objects() {
        // Create an empty OD, missing mandatory objects
        let od = ObjectDictionary::new(None);
        // CnNmtStateMachine::from_od calls od.validate_mandatory_objects internally
        // Let's test ControlledNode::new directly which also calls validate
        let result = ControlledNode::new(od, MacAddress([0; 6]));
        assert!(matches!(
            result,
            Err(PowerlinkError::ValidationError(
                "Missing common mandatory object"
            ))
        ));
    }

    #[test]
    fn test_internal_boot_sequence() {
        let mut od = get_test_od(); // Use the corrected OD
        // Create the node, which runs init() and validate() internally
        let mut node = ControlledNode::new(od, MacAddress([0; 6])).unwrap();
        // The constructor already runs run_internal_initialisation.
        assert_eq!(node.nmt_state(), NmtState::NmtNotActive);
        // Verify OD state was updated
        assert_eq!(
            node.context.core.od.read_u8(0x1F8C, 0),
            Some(NmtState::NmtNotActive as u8)
        );
    }

    #[test]
    fn test_full_boot_up_happy_path() {
        let mut od = get_test_od();
        // Create node, runs init, validate, internal_init -> NmtNotActive
        let mut node = ControlledNode::new(od, MacAddress([0; 6])).unwrap();
        assert_eq!(node.nmt_state(), NmtState::NmtNotActive);

        // NMT_CT2: Receive SoA or SoC
        node.context
            .nmt_state_machine
            .process_event(NmtEvent::SocSoAReceived, &mut node.context.core.od);
        assert_eq!(node.nmt_state(), NmtState::NmtPreOperational1);

        // NMT_CT4: Receive SoC
        node.context
            .nmt_state_machine
            .process_event(NmtEvent::SocReceived, &mut node.context.core.od);
        assert_eq!(node.nmt_state(), NmtState::NmtPreOperational2);

        // NMT_CT5: Receive EnableReadyToOperate (state doesn't change yet)
        node.context
            .nmt_state_machine
            .process_event(NmtEvent::EnableReadyToOperate, &mut node.context.core.od);
        assert_eq!(node.nmt_state(), NmtState::NmtPreOperational2);

        // NMT_CT6: Application signals completion
        node.context
            .nmt_state_machine
            .process_event(NmtEvent::CnConfigurationComplete, &mut node.context.core.od);
        assert_eq!(node.nmt_state(), NmtState::NmtReadyToOperate);

        // NMT_CT7: Receive StartNode
        node.context
            .nmt_state_machine
            .process_event(NmtEvent::StartNode, &mut node.context.core.od);
        assert_eq!(node.nmt_state(), NmtState::NmtOperational);
        assert_eq!(
            node.context.core.od.read_u8(0x1F8C, 0),
            Some(NmtState::NmtOperational as u8)
        );
    }

    #[test]
    fn test_error_handling_transition() {
        let mut od = get_test_od();
        let mut node = ControlledNode::new(od, MacAddress([0; 6])).unwrap();
        // Manually set state to Operational for test
        node.context.nmt_state_machine.current_state = NmtState::NmtOperational;
        node.context.nmt_state_machine.update_od_state(&mut node.context.core.od);

        // NMT_CT11: Trigger internal error
        node.context
            .nmt_state_machine
            .process_event(NmtEvent::Error, &mut node.context.core.od);
        assert_eq!(node.nmt_state(), NmtState::NmtPreOperational1);
        assert_eq!(
            node.context.core.od.read_u8(0x1F8C, 0),
            Some(NmtState::NmtPreOperational1 as u8)
        );
    }

    #[test]
    fn test_stop_and_restart_node() {
        let mut od = get_test_od();
        let mut node = ControlledNode::new(od, MacAddress([0; 6])).unwrap();
        // Manually set state to Operational
        node.context.nmt_state_machine.current_state = NmtState::NmtOperational;
        node.context.nmt_state_machine.update_od_state(&mut node.context.core.od);

        // NMT_CT8: Receive StopNode
        node.context
            .nmt_state_machine
            .process_event(NmtEvent::StopNode, &mut node.context.core.od);
        assert_eq!(node.nmt_state(), NmtState::NmtCsStopped);
        assert_eq!(
            node.context.core.od.read_u8(0x1F8C, 0),
            Some(NmtState::NmtCsStopped as u8)
        );

        // NMT_CT10: Receive EnterPreOperational2
        node.context
            .nmt_state_machine
            .process_event(NmtEvent::EnterPreOperational2, &mut node.context.core.od);
        assert_eq!(node.nmt_state(), NmtState::NmtPreOperational2);
        assert_eq!(
            node.context.core.od.read_u8(0x1F8C, 0),
            Some(NmtState::NmtPreOperational2 as u8)
        );
    }

    #[test]
    fn test_queue_nmt_request() {
        let od = get_test_od(); // Use corrected OD setup
        // Node creation should now succeed
        let mut node = ControlledNode::new(od, MacAddress([0; 6])).unwrap();
        assert!(node.context.pending_nmt_requests.is_empty());

        // Queue a request
        node.queue_nmt_request(NmtCommand::ResetNode, NodeId(10));
        assert_eq!(node.context.pending_nmt_requests.len(), 1);
        assert_eq!(
            node.context.pending_nmt_requests[0],
            (NmtCommand::ResetNode, NodeId(10))
        );
    }
}