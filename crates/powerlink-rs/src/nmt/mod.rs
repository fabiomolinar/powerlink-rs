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
            current_state: NMTState::Initialising,
            node_id,
        }
    }

    /// Processes an event and transitions the NMT state accordingly.
    /// This is a simplified version based on the CN state diagram (Fig. 74).
    pub fn process_event(&mut self, event: NmtEvent) {
        let next_state = match (self.current_state, event) {
            // (NMT_GT1/2/8) Initialisation events reset the state.
            (_, NmtEvent::ResetNode) => NMTState::ResetApplication,
            (_, NmtEvent::ResetCommunication) => NMTState::ResetCommunication,
            (_, NmtEvent::ResetConfiguration) => NMTState::ResetConfiguration,

            // A CN enters EPL mode upon receiving any POWERLINK frame.
            (NMTState::NotActive, NmtEvent::EnterEplMode) => NMTState::PreOperational1,
            // A timeout in NotActive moves to BasicEthernet mode.
            (NMTState::NotActive, NmtEvent::Timeout) => NMTState::BasicEthernet,
            
            // A SoC in PreOp1 moves the CN to PreOp2.
            (NMTState::PreOperational1, NmtEvent::EnterEplMode) => NMTState::PreOperational2,
            
            // The MN commands the CN to prepare for operation.
            (NMTState::PreOperational2, NmtEvent::EnableReadyToOperate) => NMTState::ReadyToOperate,
            
            // The MN commands the CN to start operation.
            (NMTState::ReadyToOperate, NmtEvent::StartNode) => NMTState::Operational,
            
            // StopNode command moves the CN to the Stopped state.
            (NMTState::PreOperational2 | NMTState::ReadyToOperate | NMTState::Operational, NmtEvent::StopNode) => NMTState::Stopped,
            
            // Any major error forces a reset back to PreOperational1.
            (NMTState::PreOperational2 | NMTState::ReadyToOperate | NMTState::Operational | NMTState::Stopped, NmtEvent::Error) => NMTState::PreOperational1,
            
            // Stay in the current state if no transition is defined.
            (current, _) => current,
        };
        self.current_state = next_state;
    }
}