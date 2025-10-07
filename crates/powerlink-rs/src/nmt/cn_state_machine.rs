use crate::types::NodeId;
use crate::od::ObjectDictionary;
use super::states::{NmtState, NmtEvent};

/// Manages the NMT state for a Controlled Node.
pub struct CnNmtStateMachine<'a> {
    pub current_state: NmtState,
    pub node_id: NodeId,
    /// A reference to the Object Dictionary is needed to read configuration.
    od: &'a ObjectDictionary,
}

impl<'a> CnNmtStateMachine<'a> {
    /// Creates a new NMT state machine for a Controlled Node.
    /// It requires an Object Dictionary to read necessary configuration, like the Node ID.
    pub fn new(od: &'a ObjectDictionary) -> Self {
        // The Node ID is read from the OD upon initialization.
        // A real implementation would handle potential errors here.
        let node_id = NodeId(1); // Placeholder

        Self {
            current_state: NmtState::NmtGsInitialising,
            node_id,
            od,
        }
    }

    /// Processes an event and transitions the NMT state according to the CN state diagram.
    /// (Reference: EPSG DS 301, Section 7.1.4, Figure 74)
    pub fn process_event(&mut self, event: NmtEvent) {
        let next_state = match (self.current_state, event) {
            // --- Reset and Initialisation Transitions ---
            (_, NmtEvent::ResetNode) => NmtState::NmtGsResetApplication,
            (_, NmtEvent::ResetCommunication) => NmtState::NmtGsResetCommunication,
            (_, NmtEvent::ResetConfiguration) => NmtState::NmtGsResetConfiguration,

            // --- CN Boot-up Sequence ---

            // (NMT_CT2) Any POWERLINK frame moves the node from NotActive to PreOp1.
            (NmtState::NmtNotActive, NmtEvent::EnterEplMode) => NmtState::NmtPreOperational1,
            // (NMT_CT3) A timeout in NotActive leads to BasicEthernet mode.
            (NmtState::NmtNotActive, NmtEvent::Timeout) => NmtState::NmtBasicEthernet,
            
            // (NMT_CT4) Receiving a SoC in PreOp1 signals the start of the isochronous phase.
            (NmtState::NmtPreOperational1, NmtEvent::EnterEplMode) => NmtState::NmtPreOperational2,
            
            // (NMT_CT5 & NMT_CT6) The MN enables the next state, and the application confirms readiness.
            (NmtState::NmtPreOperational2, NmtEvent::EnableReadyToOperate) => NmtState::NmtReadyToOperate,
            
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
            (NmtState::NmtBasicEthernet, NmtEvent::EnterEplMode) => NmtState::NmtPreOperational1,
            
            // If no specific transition is defined, remain in the current state.
            (current, _) => current,
        };
        self.current_state = next_state;
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cn_boot_up_happy_path() {
        let od = ObjectDictionary::new();
        let mut nmt = CnNmtStateMachine::new(&od);

        // Initial state after power on
        assert_eq!(nmt.current_state, NmtState::NmtGsInitialising);

        // Simulate internal auto-transitions after init
        nmt.current_state = NmtState::NmtNotActive;

        // Receive a POWERLINK frame (e.g., SoA) -> move to PreOp1
        nmt.process_event(NmtEvent::EnterEplMode);
        assert_eq!(nmt.current_state, NmtState::NmtPreOperational1);

        // Receive a SoC -> move to PreOp2
        nmt.process_event(NmtEvent::EnterEplMode);
        assert_eq!(nmt.current_state, NmtState::NmtPreOperational2);

        // Receive EnableReadyToOperate command -> move to ReadyToOperate
        nmt.process_event(NmtEvent::EnableReadyToOperate);
        assert_eq!(nmt.current_state, NmtState::NmtReadyToOperate);

        // Receive StartNode command -> move to Operational
        nmt.process_event(NmtEvent::StartNode);
        assert_eq!(nmt.current_state, NmtState::NmtOperational);
    }

    #[test]
    fn test_error_handling_transition() {
        let od = ObjectDictionary::new();
        let mut nmt = CnNmtStateMachine::new(&od);
        nmt.current_state = NmtState::NmtOperational;

        // A DLL error occurs
        nmt.process_event(NmtEvent::Error);

        // State machine should fall back to PreOperational1
        assert_eq!(nmt.current_state, NmtState::NmtPreOperational1);
    }

    #[test]
    fn test_stop_and_restart_node() {
        let od = ObjectDictionary::new();
        let mut nmt = CnNmtStateMachine::new(&od);
        nmt.current_state = NmtState::NmtOperational;

        // MN sends StopNode command
        nmt.process_event(NmtEvent::StopNode);
        assert_eq!(nmt.current_state, NmtState::NmtCsStopped);
        
        // MN sends EnterPreOperational2 to bring it back
        nmt.process_event(NmtEvent::EnterPreOperational2);
        assert_eq!(nmt.current_state, NmtState::NmtPreOperational2);
    }
}