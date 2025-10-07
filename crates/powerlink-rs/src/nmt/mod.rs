pub mod states;
pub mod cn_state_machine;
pub mod flags;

use crate::types::{NodeId};
use self::states::{NmtState, NmtEvent};

/// Manages the NMT state for a POWERLINK node.
pub struct NmtStateMachine {
    pub current_state: NmtState,
    pub node_id: NodeId,
    // Other fields like heartbeat timers can be added here.
}

impl NmtStateMachine {
    /// Creates a new NMT state machine for a node.
    pub fn new(node_id: NodeId) -> Self {
        // All nodes start in the Initialising state after power on.
        Self {
            current_state: NmtState::NmtGsInitialising,
            node_id,
        }
    }

    /// Processes an event and transitions the NMT state accordingly.
    ///
    /// This is a simplified version based on the CN state diagram (Fig. 74).
    pub fn process_event(&mut self, event: NmtEvent) {
        let next_state = match (self.current_state, event) {
            // (NMT_GT1/2/8) Initialisation events reset the state.
            (_, NmtEvent::ResetNode) => NmtState::NmtGsResetApplication,
            (_, NmtEvent::ResetCommunication) => NmtState::NmtGsResetCommunication,
            (_, NmtEvent::ResetConfiguration) => NmtState::NmtGsResetConfiguration,

            // A CN enters EPL mode upon receiving any POWERLINK frame.
            (NmtState::NmtNotActive, NmtEvent::EnterEplMode) => NmtState::NmtPreOperational1,
            // A timeout in NotActive moves to BasicEthernet mode.
            (NmtState::NmtNotActive, NmtEvent::Timeout) => NmtState::NmtBasicEthernet,
            
            // A SoC in PreOp1 moves the CN to PreOp2.
            (NmtState::NmtPreOperational1, NmtEvent::EnterEplMode) => NmtState::NmtPreOperational2,
            
            // The MN commands the CN to prepare for operation.
            (NmtState::NmtPreOperational2, NmtEvent::EnableReadyToOperate) => NmtState::NmtReadyToOperate,
            
            // The MN commands the CN to start operation.
            (NmtState::NmtReadyToOperate, NmtEvent::StartNode) => NmtState::NmtOperational,
            
            // StopNode command moves the CN to the Stopped state.
            (NmtState::NmtPreOperational2 | NmtState::NmtReadyToOperate | NmtState::NmtOperational, NmtEvent::StopNode) => NmtState::NmtCsStopped,
            
            // Any major error forces a reset back to PreOperational1.
            (NmtState::NmtPreOperational2 | NmtState::NmtReadyToOperate | NmtState::NmtOperational | NmtState::NmtCsStopped, NmtEvent::Error) => NmtState::NmtPreOperational1,
            
            // Stay in the current state if no transition is defined.
            (current, _) => current,
        };
        self.current_state = next_state;
    }
}