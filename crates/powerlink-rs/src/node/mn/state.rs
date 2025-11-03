// crates/powerlink-rs/src/node/mn/state.rs
use crate::ErrorHandler;
use crate::frame::error::{DllErrorManager, ErrorCounters, LoggingErrorHandler, MnErrorCounters};
use crate::frame::{DllMsEvent, DllMsStateMachine, PowerlinkFrame};
use crate::nmt::events::NmtCommand;
use crate::nmt::mn_state_machine::MnNmtStateMachine;
use crate::node::{CoreNodeContext, NodeContext, PdoHandler};
use crate::od::ObjectDictionary;
use crate::sdo::transport::AsndTransport;
#[cfg(feature = "sdo-udp")]
use crate::sdo::transport::UdpTransport;
use crate::types::NodeId;
use alloc::collections::{BTreeMap, BinaryHeap};
use alloc::vec::Vec;
use core::cmp::Ordering;

/// Holds the complete state for a Managing Node.
pub struct MnContext<'s> {
    pub core: CoreNodeContext<'s>, // Use CoreNodeContext for shared state
    pub nmt_state_machine: MnNmtStateMachine,
    pub dll_state_machine: DllMsStateMachine,
    // dll_error_manager is separated due to its generic parameters
    pub dll_error_manager: DllErrorManager<MnErrorCounters, LoggingErrorHandler>,
    /// SDO transport handler for ASnd.
    pub asnd_transport: AsndTransport,
    /// SDO transport handler for UDP.
    #[cfg(feature = "sdo-udp")]
    pub udp_transport: UdpTransport,
    pub cycle_time_us: u64,
    // ... rest of the fields remain the same ...
    pub multiplex_cycle_len: u8,
    pub multiplex_assign: BTreeMap<NodeId, u8>,
    pub current_multiplex_cycle: u8,
    pub node_info: BTreeMap<NodeId, CnInfo>,
    pub mandatory_nodes: Vec<NodeId>,
    pub isochronous_nodes: Vec<NodeId>,
    pub async_only_nodes: Vec<NodeId>,
    pub next_isoch_node_idx: usize,
    pub current_phase: CyclePhase,
    pub current_polled_cn: Option<NodeId>,
    pub async_request_queue: BinaryHeap<AsyncRequest>,
    /// A high-priority queue for sending StatusRequests to CNs that need an ER flag.
    pub pending_er_requests: Vec<NodeId>,
    pub pending_status_requests: Vec<NodeId>,
    pub pending_nmt_commands: Vec<(NmtCommand, NodeId)>,
    pub mn_async_send_queue: Vec<PowerlinkFrame>,
    pub pending_sdo_client_requests: Vec<(NodeId, Vec<u8>)>,
    pub last_ident_poll_node_id: NodeId,
    pub last_status_poll_node_id: NodeId,
    pub next_tick_us: Option<u64>,
    pub pending_timeout_event: Option<DllMsEvent>,
    pub current_cycle_start_time_us: u64,
    pub initial_operational_actions_done: bool,
}

impl<'s> PdoHandler<'s> for MnContext<'s> {
    fn od(&mut self) -> &mut ObjectDictionary<'s> {
        &mut self.core.od
    }

    fn dll_error_manager(&mut self) -> &mut DllErrorManager<impl ErrorCounters, impl ErrorHandler> {
        &mut self.dll_error_manager
    }
}

impl<'s> NodeContext<'s> for MnContext<'s> {
    fn is_cn(&self) -> bool {
        false
    }
    fn core(&self) -> &CoreNodeContext<'s> {
        &self.core
    }
    fn core_mut(&mut self) -> &mut CoreNodeContext<'s> {
        &mut self.core
    }
    fn nmt_state_machine(&self) -> &dyn crate::nmt::NmtStateMachine {
        &self.nmt_state_machine
    }
}

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
    /// Flag indicating the `CHECK_COMMUNICATION` step has passed.
    pub communication_ok: bool,
}

impl Default for CnInfo {
    fn default() -> Self {
        Self {
            state: CnState::Unknown,
            // Both flags start as false, as no error has been signaled or acknowledged.
            en_flag: false,
            ea_flag: false,
            communication_ok: false,
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