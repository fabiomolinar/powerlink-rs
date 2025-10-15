// In crates/powerlink-rs/src/nmt/cn_state_machine.rs

use crate::frame::DllError;
use crate::od::{ObjectDictionary, ObjectValue};
use crate::types::NodeId;
use crate::PowerlinkError;
use super::flags::FeatureFlags;
use super::states::{NmtEvent, NmtState};
use alloc::vec::Vec;

/// Manages the NMT state for a Controlled Node.
// No longer needs lifetime parameters.
pub struct CnNmtStateMachine {
    pub current_state: NmtState,
    pub node_id: NodeId,
    pub feature_flags: FeatureFlags,
    pub basic_ethernet_timeout: u32,
}

impl CnNmtStateMachine {
    /// Creates a new NMT state machine with pre-validated parameters.
    pub fn new(
        node_id: NodeId,
        feature_flags: FeatureFlags,
        basic_ethernet_timeout: u32,
    ) -> Self {
        Self {
            current_state: NmtState::NmtGsInitialising,
            node_id,
            feature_flags,
            basic_ethernet_timeout,
        }
    }

    /// A fallible constructor that reads its configuration from an Object Dictionary.
    pub fn from_od(od: &ObjectDictionary) -> Result<Self, PowerlinkError> {
        // Read Node ID from OD entry 0x1F93, sub-index 1.
        let node_id_val = od.read(0x1F93, 1).ok_or(PowerlinkError::ObjectNotFound)?;
        let node_id = if let ObjectValue::Unsigned8(val) = &*node_id_val {
            NodeId::try_from(*val)?
        } else {
            return Err(PowerlinkError::TypeMismatch);
        };

        // Read Feature Flags from OD entry 0x1F82, sub-index 0.
        let feature_flags_val = od.read(0x1F82, 0).ok_or(PowerlinkError::ObjectNotFound)?;
        let feature_flags = if let ObjectValue::Unsigned32(val) = &*feature_flags_val {
            FeatureFlags::from_bits_truncate(*val)
        } else {
            return Err(PowerlinkError::TypeMismatch);
        };

        // Read Basic Ethernet Timeout from OD entry 0x1F99, sub-index 0.
        let basic_ethernet_timeout_val = od.read(0x1F99, 0).ok_or(PowerlinkError::ObjectNotFound)?;
        let basic_ethernet_timeout = if let ObjectValue::Unsigned32(val) = &*basic_ethernet_timeout_val {
            *val
        } else {
            return Err(PowerlinkError::TypeMismatch);
        };

        Ok(Self::new(node_id, feature_flags, basic_ethernet_timeout))
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
    use crate::od::{Object, ObjectEntry};
    use crate::od::AccessType;
    use alloc::vec;

    fn get_test_od() -> ObjectDictionary<'static> {
        let mut od = ObjectDictionary::new(None);
        od.insert(0x1F93, ObjectEntry {
            object: Object::Record(vec![
                ObjectValue::Unsigned8(42),
                ObjectValue::Boolean(0),
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

    // Helper for creating a state machine for tests
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
        let od = ObjectDictionary::new(None);
        let result = CnNmtStateMachine::from_od(&od);
        assert_eq!(result.err(), Some(PowerlinkError::ObjectNotFound));
    }

    #[test]
    fn test_internal_boot_sequence() {
        let mut nmt = get_test_nmt();
        assert_eq!(nmt.current_state, NmtState::NmtGsInitialising);
        nmt.run_internal_initialisation();
        assert_eq!(nmt.current_state, NmtState::NmtNotActive);
    }

    #[test]
    fn test_full_boot_up_happy_path() {
        let mut nmt = get_test_nmt();
        nmt.current_state = NmtState::NmtNotActive;
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
        let mut nmt = get_test_nmt();
        nmt.current_state = NmtState::NmtOperational;
        nmt.process_event(NmtEvent::Error);
        assert_eq!(nmt.current_state, NmtState::NmtPreOperational1);
    }

    #[test]
    fn test_stop_and_restart_node() {
        let mut nmt = get_test_nmt();
        nmt.current_state = NmtState::NmtOperational;
        nmt.process_event(NmtEvent::StopNode);
        assert_eq!(nmt.current_state, NmtState::NmtCsStopped);
        nmt.process_event(NmtEvent::EnterPreOperational2);
        assert_eq!(nmt.current_state, NmtState::NmtPreOperational2);
    }
}