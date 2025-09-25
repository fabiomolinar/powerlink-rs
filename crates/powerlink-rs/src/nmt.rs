use crate::types::{NodeId, UNSIGNED16};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NMTState {
}

pub struct NMTStateMachine {
    pub current_state: NMTState,
    pub previous_state: NMTState,
    pub node_id: NodeId,
    pub heartbeat_time: UNSIGNED16, // in ms
    pub is_operational: bool,
}