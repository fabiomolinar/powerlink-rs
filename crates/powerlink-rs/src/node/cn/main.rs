// crates/powerlink-rs/src/node/cn/main.rs
use super::events;
use super::state::CnContext;
use crate::PowerlinkError;
use crate::frame::basic::MacAddress;
use crate::frame::error::{CnErrorCounters, DllErrorManager, LoggingErrorHandler};
use crate::frame::{DllError, NmtAction, deserialize_frame};
use crate::nmt::cn_state_machine::CnNmtStateMachine;
use crate::nmt::events::{NmtCommand, NmtEvent};
use crate::nmt::state_machine::NmtStateMachine;
use crate::nmt::states::NmtState;
use crate::node::{CoreNodeContext, Node, NodeAction};
use crate::od::ObjectDictionary;
use crate::sdo::transport::AsndTransport;
#[cfg(feature = "sdo-udp")]
use crate::sdo::transport::UdpTransport;
use crate::sdo::{SdoClient, SdoServer};
use crate::types::{C_ADR_MN_DEF_NODE_ID, NodeId};
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use log::{debug, error, info, trace, warn};

// --- Add imports for UDP SDO ---
#[cfg(feature = "sdo-udp")]
use crate::sdo::{
    server::SdoClientInfo,
    transport::SdoTransport,
    udp::deserialize_sdo_udp_payload,
};
#[cfg(feature = "sdo-udp")]
use crate::types::IpAddress;
// --- End of imports ---

const OD_IDX_ERROR_REGISTER: u16 = 0x1001;

/// Represents a complete POWERLINK Controlled Node (CN).
/// This struct is a thin wrapper around a context object that holds all state.
pub struct ControlledNode<'s> {
    pub context: CnContext<'s>,
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
                dll_error_manager: DllErrorManager::new(
                    CnErrorCounters::new(),
                    LoggingErrorHandler,
                ),
                asnd_transport: AsndTransport,
                #[cfg(feature = "sdo-udp")]
                udp_transport: UdpTransport,
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
        self.context
            .core
            .queue_sdo_request(NodeId(C_ADR_MN_DEF_NODE_ID), payload);
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
    fn process_ethernet_frame(&mut self, buffer: &[u8], current_time_us: u64) -> NodeAction {
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
                    let current_err_reg = self
                        .context
                        .core
                        .od
                        .read_u8(OD_IDX_ERROR_REGISTER, 0)
                        .unwrap_or(0);
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
                        .unwrap_or_else(|e| {
                            error!("[CN] Failed to update Error Register: {:?}", e)
                        });
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

    /// Processes a UDP datagram payload for SDO over UDP.
    #[cfg(feature = "sdo-udp")]
    fn process_udp_datagram(
        &mut self,
        buffer: &[u8],
        source_ip: IpAddress,
        source_port: u16,
        current_time_us: u64,
    ) -> NodeAction {
        debug!(
            "[CN] Received UDP datagram ({} bytes) from {}:{}",
            buffer.len(),
            core::net::Ipv4Addr::from(source_ip),
            source_port
        );

        // 1. Deserialize the SDO payload from the UDP datagram
        let (seq_header, cmd) = match deserialize_sdo_udp_payload(buffer) {
            Ok((seq, cmd)) => (seq, cmd),
            Err(e) => {
                warn!("[CN] Failed to deserialize SDO/UDP payload: {:?}", e);
                // Cannot send a response if we can't parse the request
                return NodeAction::NoAction;
            }
        };

        // 2. Define the client info for the SDO server
        let client_info = SdoClientInfo::Udp {
            source_ip,
            source_port,
        };

        // 3. Re-serialize the SDO payload (SeqHdr + Cmd) for the SdoServer.
        let mut sdo_payload = vec![0u8; buffer.len()]; // Max possible size
        let seq_len = seq_header.serialize(&mut sdo_payload).unwrap_or(0);
        let cmd_len = cmd.serialize(&mut sdo_payload[seq_len..]).unwrap_or(0);
        let total_sdo_len = seq_len + cmd_len;
        sdo_payload.truncate(total_sdo_len);

        // 4. Handle the SDO command
        match self.context.core.sdo_server.handle_request(
            &sdo_payload,
            client_info,
            &mut self.context.core.od,
            current_time_us,
        ) {
            Ok(response_data) => {
                // 5. Build and return the UDP response action
                match self
                    .context
                    .udp_transport
                    .build_response(response_data, &self.context)
                {
                    Ok(action) => action,
                    Err(e) => {
                        error!("[CN] Failed to build SDO/UDP response: {:?}", e);
                        NodeAction::NoAction
                    }
                }
            }
            Err(e) => {
                error!("[CN] SDO server error (UDP): {:?}", e);
                NodeAction::NoAction
            }
        }
    }

    /// Internal tick handler, moved from the trait implementation.
    fn tick(&mut self, current_time_us: u64) -> NodeAction {
        events::process_tick(&mut self.context, current_time_us)
    }
}

impl<'s> Node for ControlledNode<'s> {
    #[cfg(feature = "sdo-udp")]
    fn run_cycle(
        &mut self,
        ethernet_frame: Option<&[u8]>,
        udp_datagram: Option<(&[u8], IpAddress, u16)>,
        current_time_us: u64,
    ) -> NodeAction {
        // --- Priority 1: Ethernet Frames ---
        if let Some(buffer) = ethernet_frame {
            // Check for POWERLINK EtherType
            if buffer.len() >= 14
                && buffer[12..14] == crate::types::C_DLL_ETHERTYPE_EPL.to_be_bytes()
            {
                let action = self.process_ethernet_frame(buffer, current_time_us);
                if action != NodeAction::NoAction {
                    return action;
                }
            }
            // Ignore non-POWERLINK Ethernet frames
        }

        // --- Priority 2: UDP Datagrams ---
        if let Some((buffer, ip, port)) = udp_datagram {
            let action = self.process_udp_datagram(buffer, ip, port, current_time_us);
            if action != NodeAction::NoAction {
                return action;
            }
        }

        // --- Priority 3: Internal Ticks ---
        self.tick(current_time_us)
    }

    #[cfg(not(feature = "sdo-udp"))]
    fn run_cycle(
        &mut self,
        ethernet_frame: Option<&[u8]>,
        current_time_us: u64,
    ) -> NodeAction {
        // --- Priority 1: Ethernet Frames ---
        if let Some(buffer) = ethernet_frame {
            // Check for POWERLINK EtherType
            if buffer.len() >= 14
                && buffer[12..14] == crate::types::C_DLL_ETHERTYPE_EPL.to_be_bytes()
            {
                let action = self.process_ethernet_frame(buffer, current_time_us);
                if action != NodeAction::NoAction {
                    return action;
                }
            }
            // Ignore non-POWERLINK Ethernet frames
        }

        // --- Priority 2: Internal Ticks ---
        self.tick(current_time_us)
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
        let od = get_test_od(); // Use the corrected OD
        // Create the node, which runs init() and validate() internally
        let node = ControlledNode::new(od, MacAddress([0; 6])).unwrap();
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
        let od = get_test_od();
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
        let od = get_test_od();
        let mut node = ControlledNode::new(od, MacAddress([0; 6])).unwrap();
        // Manually set state to Operational for test
        node.context.nmt_state_machine.current_state = NmtState::NmtOperational;
        node.context
            .nmt_state_machine
            .update_od_state(&mut node.context.core.od);

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
        let od = get_test_od();
        let mut node = ControlledNode::new(od, MacAddress([0; 6])).unwrap();
        // Manually set state to Operational
        node.context.nmt_state_machine.current_state = NmtState::NmtOperational;
        node.context
            .nmt_state_machine
            .update_od_state(&mut node.context.core.od);

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
