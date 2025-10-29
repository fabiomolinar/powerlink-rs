use crate::types::NodeId;
use core::cmp::Ordering;

/// Internal state tracking for each configured CN.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd)]
pub enum CnState {
    /// Initial state, node is configured but not heard from.
    Unknown,
    /// Node has responded to IdentRequest.
    Identified,
    /// Node is in PreOp2 or ReadyToOperate (signaled via PRes/StatusResponse).
    PreOperational,
    /// Node is in Operational (signaled via PRes/StatusResponse).
    Operational,
    /// Node is stopped (signaled via PRes/StatusResponse).
    Stopped,
    /// Node missed a PRes or timed out, or other communication error occurred.
    Missing,
}

/// A struct holding all state information for a single CN, as tracked by the MN.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CnInfo {
    /// The last known NMT state of the CN.
    pub state: CnState,
    /// The last known EN (Exception New) flag received from the CN.
    pub en_flag: bool,
    /// The last EA (Exception Acknowledge) flag sent *to* the CN by the MN.
    pub ea_flag: bool,
}

impl Default for CnInfo {
    fn default() -> Self {
        Self {
            state: CnState::Unknown,
            // Both flags start as false, as no error has been signaled or acknowledged.
            en_flag: false,
            ea_flag: false,
        }
    }
}


/// Tracks the current phase within the POWERLINK cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CyclePhase {
    /// Waiting for next cycle start or PreOp1 SoA
    Idle,
    /// SoC has been sent, start isochronous phase
    SoCSent,
    /// PReq sent, waiting for PRes or timeout
    IsochronousPReq,
    /// All isochronous nodes polled
    IsochronousDone,
    /// SoA sent, maybe waiting for ASnd or timeout
    AsynchronousSoA,
    /// SoA sent to self, waiting to send ASnd(NMT)
    AwaitingMnAsyncSend,
}

/// Represents a pending asynchronous transmission request from a CN.
#[derive(Debug, Clone, Copy, Eq)]
pub struct AsyncRequest {
    pub node_id: NodeId,
    /// Higher value = higher priority (7 = NMT)
    pub priority: u8,
}

// Implement Ord and PartialOrd for AsyncRequest to use it in BinaryHeap (Max Heap)
impl Ord for AsyncRequest {
    fn cmp(&self, other: &Self) -> Ordering {
        // Compare priorities directly
        self.priority.cmp(&other.priority)
    }
}

impl PartialOrd for AsyncRequest {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for AsyncRequest {
    fn eq(&self, other: &Self) -> bool {
        // Equal only if both node_id and priority match.
        self.priority == other.priority && self.node_id == other.node_id
    }
}