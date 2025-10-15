use crate::frame::DllError;
use crate::types::NodeId;
use crate::od::{ObjectDictionary, ObjectValue};
use crate::PowerlinkError;
use super::states::{NmtState, NmtEvent};
use super::flags::FeatureFlags;
use alloc::vec::Vec;

/// Manages the NMT state for a Controlled Node.
pub struct CnNmtStateMachine<'od, 's> {
    pub current_state: NmtState,
    pub node_id: NodeId,
    /// Cached feature flags from OD 0x1F82.
    pub feature_flags: FeatureFlags,
    /// Cached timeout for Basic Ethernet transition from OD 0x1F99.
    pub basic_ethernet_timeout: u32,
    /// A reference to the Object Dictionary.
    od: &'od ObjectDictionary<'s>,
}

impl<'od, 's> CnNmtStateMachine<'od, 's> {
    /// Creates a new NMT state machine for a Controlled Node.
    /// This is now fallible, as it must successfully read the Node ID from the OD.
    pub fn new(od: &'od ObjectDictionary<'s>) -> Result<Self, PowerlinkError> {
        // Read Node ID from OD entry 0x1F93, sub-index 1.
        let node_id_val = od.read(0x1F93, 1)
            .ok_or(PowerlinkError::ObjectNotFound)?;
        // Dereference the Cow to access the inner ObjectValue.
        let node_id = if let ObjectValue::Unsigned8(val) = &*node_id_val {
            NodeId::try_from(*val)?
        } else {
            return Err(PowerlinkError::TypeMismatch);
        };

        // Read Feature Flags from OD entry 0x1F82, sub-index 0.
        let feature_flags_val = od.read(0x1F82, 0)
            .ok_or(PowerlinkError::ObjectNotFound)?;
        let feature_flags = if let ObjectValue::Unsigned32(val) = &*feature_flags_val {
            FeatureFlags::from_bits_truncate(*val)
        } else {
            return Err(PowerlinkError::TypeMismatch);
        };


        // Read Basic Ethernet Timeout from OD entry 0x1F99, sub-index 0.
        let basic_ethernet_timeout = od.read(0x1F99, 0)
            .ok_or(PowerlinkError::ObjectNotFound)?;
        let basic_ethernet_timeout = if let ObjectValue::Unsigned32(val) = &*basic_ethernet_timeout {
            *val
        } else {
            return Err(PowerlinkError::TypeMismatch);
        };

        Ok(Self {
            current_state: NmtState::NmtGsInitialising,
            node_id,
            feature_flags,
            basic_ethernet_timeout,
            od,
        })
    }
    
    /// Resets the state machine to a specific reset state.
    pub fn reset(&mut self, event: NmtEvent) {
        match event {
            NmtEvent::Reset => self.current_state = NmtState::NmtGsInitialising,
            NmtEvent::ResetNode => self.current_state = NmtState::NmtGsResetApplication,
            NmtEvent::ResetCommunication => self.current_state = NmtState::NmtGsResetCommunication,
            NmtEvent::ResetConfiguration => self.current_state = NmtState::NmtGsResetConfiguration,
            _ => {}, // Ignore other events
        }
    }

    /// Handles automatic, internal state transitions that don't require an external event.
    /// This should be called in a loop after `process_event`.
    pub fn run_internal_initialisation(&mut self) {
        let mut transition = true;
        while transition {
            let next_state = match self.current_state {
                // After basic init, automatically move to reset the application.
                NmtState::NmtGsInitialising => NmtState::NmtGsResetApplication,
                // After app reset, automatically move to reset comms.
                NmtState::NmtGsResetApplication => NmtState::NmtGsResetCommunication,
                // After comms reset, automatically move to reset config.
                NmtState::NmtGsResetCommunication => NmtState::NmtGsResetConfiguration,
                // After config reset, the node is ready to listen on the network.
                NmtState::NmtGsResetConfiguration => NmtState::NmtNotActive,
                // No other states have automatic transitions.
                _ => {
                    transition = false; // Stop the loop
                    self.current_state
                }
            };
            self.current_state = next_state;
        }
    }

    /// Processes an external event and transitions the NMT state accordingly.
    pub fn process_event(&mut self, event: NmtEvent) -> Option<Vec<DllError>> {
        let mut errors: Vec<DllError> = Vec::new();
        let next_state = match (self.current_state, event) {
            // --- Reset and Initialisation Transitions ---
            (_, NmtEvent::Reset) => NmtState::NmtGsInitialising,
            (_, NmtEvent::ResetNode) => NmtState::NmtGsResetApplication,
            (_, NmtEvent::ResetCommunication) => NmtState::NmtGsResetCommunication,
            (_, NmtEvent::ResetConfiguration) => NmtState::NmtGsResetConfiguration,

            // --- CN Boot-up Sequence ---

            // (NMT_CT2) Any POWERLINK frame moves the node from NotActive to PreOp1.
            (NmtState::NmtNotActive, NmtEvent::SocSoAReceived) => NmtState::NmtPreOperational1,
            // (NMT_CT3) A timeout in NotActive leads to BasicEthernet mode.
            (NmtState::NmtNotActive, NmtEvent::Timeout) => NmtState::NmtBasicEthernet,
            
            // (NMT_CT4) Receiving a SoC in PreOp1 signals the start of the isochronous phase.
            (NmtState::NmtPreOperational1, NmtEvent::SocReceived) => NmtState::NmtPreOperational2,
            
            // (NMT_CT5) The MN enables the next state, and the application confirms readiness.
            (NmtState::NmtPreOperational2, NmtEvent::EnableReadyToOperate) => NmtState::NmtPreOperational2,
            // (NMT_CT6) The MN enables the next state, and the application confirms readiness.
            (NmtState::NmtPreOperational2, NmtEvent::CnConfigurationComplete) => NmtState::NmtReadyToOperate,            
            // (NMT_CT7) The MN commands the CN to start full operation.
            (NmtState::NmtReadyToOperate, NmtEvent::StartNode) => NmtState::NmtOperational,
            
            // --- Operational State Transitions ---

            // (NMT_CT8) The MN can stop a node from several states.
            (NmtState::NmtPreOperational2 | NmtState::NmtReadyToOperate | NmtState::NmtOperational, NmtEvent::StopNode) => NmtState::NmtCsStopped,
            
            // (NMT_CT9) The MN can command a node to return to PreOp2.
            (NmtState::NmtOperational, NmtEvent::EnterPreOperational2) => NmtState::NmtPreOperational2,
            // (NMT_CT10) The MN can bring a stopped node back to PreOp2.
            (NmtState::NmtCsStopped, NmtEvent::EnterPreOperational2) => NmtState::NmtPreOperational2,
            // (NMT_CT11) A critical error in any cyclic state forces a reset to PreOp1.
            (NmtState::NmtPreOperational2 | NmtState::NmtReadyToOperate | NmtState::NmtOperational | NmtState::NmtCsStopped, NmtEvent::Error) => NmtState::NmtPreOperational1,

            // (NMT_CT12) Receiving a POWERLINK frame while in BasicEthernet forces a return to PreOp1.
            (NmtState::NmtBasicEthernet, NmtEvent::PowerlinkFrameReceived) => NmtState::NmtPreOperational1,
            
            // If no specific transition is defined, remain in the current state.
            (current, _) => {
                errors.push(DllError::UnexpectedEventInState { state: current as u8, event: event as u8 });
                current
            },
        };
        self.current_state = next_state;

        if errors.is_empty() {
            None
        } else {
            Some(errors)
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::od::{ObjectDictionary, Object, ObjectValue, ObjectEntry, AccessType};
    use alloc::vec;    

    // Helper to create a test OD with all mandatory values.
    fn get_test_od() -> ObjectDictionary<'static> {
        let mut od = ObjectDictionary::new(None);
        
        od.insert(0x1F93, ObjectEntry {
            object: Object::Record(vec![
                ObjectValue::Unsigned8(42), // Node ID at sub-index 1
                ObjectValue::Boolean(0),    // NodeIDByHW_BOOL at sub-index 2
            ]),
            name: "NMT_EPLNodeID_REC",
            access: AccessType::ReadWrite,
        });

        let flags = FeatureFlags::ISOCHRONOUS | FeatureFlags::SDO_ASND | FeatureFlags::SDO_UDP;
        od.insert(0x1F82, ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned32(flags.0)),
            name: "NMT_FeatureFlags_U32",
            access: AccessType::Constant,
        });

        od.insert(0x1F99, ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned32(5_000_000)),
            name: "NMT_CNBasicEthernetTimeout_U32",
            access: AccessType::ReadWrite,
        });
        
        od
    }

    #[test]
    fn test_new_reads_node_id() {
        let od = get_test_od();
        let nmt = CnNmtStateMachine::new(&od).unwrap();
        assert_eq!(nmt.node_id, NodeId(42));
    }

    #[test]
    fn test_internal_boot_sequence() {
        let od = get_test_od();
        let mut nmt = CnNmtStateMachine::new(&od).unwrap();

        // Starts in Initialising
        assert_eq!(nmt.current_state, NmtState::NmtGsInitialising);

        // Run the automatic boot-up sequence
        nmt.run_internal_initialisation();
        
        // Should end up in NotActive, ready for network events.
        assert_eq!(nmt.current_state, NmtState::NmtNotActive);
    }

    #[test]
    fn test_full_boot_up_happy_path() {
        let od = get_test_od();
        let mut nmt = CnNmtStateMachine::new(&od).unwrap();

        nmt.run_internal_initialisation(); // -> NotActive
        assert_eq!(nmt.current_state, NmtState::NmtNotActive);

        nmt.process_event(NmtEvent::SocSoAReceived);
        assert_eq!(nmt.current_state, NmtState::NmtPreOperational1);

        nmt.process_event(NmtEvent::SocReceived);
        assert_eq!(nmt.current_state, NmtState::NmtPreOperational2);

        nmt.process_event(NmtEvent::EnableReadyToOperate);
        assert_eq!(nmt.current_state, NmtState::NmtPreOperational2);

        nmt.process_event(NmtEvent::CnConfigurationComplete);
        assert_eq!(nmt.current_state, NmtState::NmtReadyToOperate);

        nmt.process_event(NmtEvent::StartNode);
        assert_eq!(nmt.current_state, NmtState::NmtOperational);
    }

    #[test]
    fn test_error_handling_transition() {
        let od = get_test_od();
        let mut nmt = CnNmtStateMachine::new(&od).unwrap();
        nmt.current_state = NmtState::NmtOperational;

        // A DLL error occurs
        nmt.process_event(NmtEvent::Error);

        // State machine should fall back to PreOperational1
        assert_eq!(nmt.current_state, NmtState::NmtPreOperational1); 
    }

    #[test]
    fn test_stop_and_restart_node() {
        let od = get_test_od();
        let mut nmt = CnNmtStateMachine::new(&od).unwrap();
        nmt.current_state = NmtState::NmtOperational;

        // MN sends StopNode command
        nmt.process_event(NmtEvent::StopNode);
        assert_eq!(nmt.current_state, NmtState::NmtCsStopped);
        
        // MN sends EnterPreOperational2 to bring it back
        nmt.process_event(NmtEvent::EnterPreOperational2);
        assert_eq!(nmt.current_state, NmtState::NmtPreOperational2);
    }

    #[test]
    fn test_new_fails_if_od_is_missing_nodeid() {
        // Create an empty OD without the required Node ID object.
        let od = ObjectDictionary::new(None);
        let result = CnNmtStateMachine::new(&od);
        assert_eq!(result.err(), Some(PowerlinkError::ObjectNotFound));
    }

    #[test]
    fn test_new_reads_od_parameters() {
        let od = get_test_od();
        let nmt = CnNmtStateMachine::new(&od).unwrap();
        
        assert_eq!(nmt.node_id, NodeId(42));
        // UPDATED: Test for a specific flag.
        assert!(nmt.feature_flags.contains(FeatureFlags::SDO_ASND));
        assert!(nmt.feature_flags.contains(FeatureFlags::SDO_UDP));
        assert!(!nmt.feature_flags.contains(FeatureFlags::ROUTING_TYPE_1));
        assert_eq!(nmt.basic_ethernet_timeout, 5_000_000);
    }
}