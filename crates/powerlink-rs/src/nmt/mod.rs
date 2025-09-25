use crate::types::{NodeId, UNSIGNED16};
use self::states::{NMTState, NmtEvent};

pub mod states;

/// Manages the NMT state for a POWERLINK node.
pub struct NMTStateMachine {
    pub current_state: NMTState,
    pub node_id: NodeId,
    // Other fields like heartbeat timers can be added here.
}

impl NMTStateMachine {
    /// Creates a new NMT state machine for a node.
    pub fn new(node_id: NodeId) -> Self {
        // All nodes start in the Initialising state after power on.
        Self {
            current_state: NMTState::NMT_GS_INITIALISING,
            node_id,
        }
    }

    /// Processes an event and transitions the NMT state accordingly.
    /// This is a simplified version based on the CN state diagram (Fig. 74).
    pub fn process_event(&mut self, event: NmtEvent) {
        let next_state = match (self.current_state, event) {
            // (NMT_GT1/2/8) Initialisation events reset the state.
            (_, NmtEvent::ResetNode) => NMTState::NMT_GS_RESET_APPLICATION,
            (_, NmtEvent::ResetCommunication) => NMTState::NMT_GS_RESET_COMMUNICATION,
            (_, NmtEvent::ResetConfiguration) => NMTState::NMT_GS_RESET_CONFIGURATION,

            // A CN enters EPL mode upon receiving any POWERLINK frame.
            (NMTState::NMT_CS_NOT_ACTIVE, NmtEvent::EnterEplMode) => NMTState::NMT_CS_PRE_OPERATIONAL_1,
            // A timeout in NotActive moves to BasicEthernet mode.
            (NMTState::NMT_CS_NOT_ACTIVE, NmtEvent::Timeout) => NMTState::NMT_CS_BASIC_ETHERNET,
            
            // A SoC in PreOp1 moves the CN to PreOp2.
            (NMTState::NMT_CS_PRE_OPERATIONAL_1, NmtEvent::EnterEplMode) => NMTState::NMT_CS_PRE_OPERATIONAL_2,
            
            // The MN commands the CN to prepare for operation.
            (NMTState::NMT_CS_PRE_OPERATIONAL_2, NmtEvent::EnableReadyToOperate) => NMTState::NMT_CS_READY_TO_OPERATE,
            
            // The MN commands the CN to start operation.
            (NMTState::NMT_CS_READY_TO_OPERATE, NmtEvent::StartNode) => NMTState::NMT_CS_OPERATIONAL,
            
            // StopNode command moves the CN to the Stopped state.
            (NMTState::NMT_CS_PRE_OPERATIONAL_2 | NMTState::NMT_CS_READY_TO_OPERATE | NMTState::NMT_CS_OPERATIONAL, NmtEvent::StopNode) => NMTState::NMT_CS_STOPPED,
            
            // Any major error forces a reset back to PreOperational1.
            (NMTState::NMT_CS_PRE_OPERATIONAL_2 | NMTState::NMT_CS_READY_TO_OPERATE | NMTState::NMT_CS_OPERATIONAL | NMTState::NMT_CS_STOPPED, NmtEvent::Error) => NMTState::NMT_CS_PRE_OPERATIONAL_1,
            
            // Stay in the current state if no transition is defined.
            (current, _) => current,
        };
        self.current_state = next_state;
    }
}