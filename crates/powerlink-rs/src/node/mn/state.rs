use crate::ErrorHandler;
use crate::frame::basic::MacAddress;
use crate::frame::error::{DllErrorManager, ErrorCounters, LoggingErrorHandler, MnErrorCounters};
use crate::frame::{DllMsEvent, DllMsStateMachine, PowerlinkFrame, ServiceId}; // Import ServiceId
use crate::nmt::events::MnNmtCommandRequest;
use crate::nmt::mn_state_machine::MnNmtStateMachine;
use crate::nmt::states::NmtState;
use crate::node::{CoreNodeContext, NodeContext, PdoHandler};
use crate::sdo::client_manager::SdoClientManager;
use crate::sdo::transport::AsndTransport;
#[cfg(feature = "sdo-udp")]
use crate::sdo::transport::UdpTransport;
use crate::types::{IpAddress, NodeId};
use alloc::collections::{BTreeMap, BinaryHeap};
use alloc::string::String; // Import String
use alloc::vec::Vec;
use core::cmp::Ordering;

/// Represents the data payload for an NMT Managing Command.
/// (Reference: EPSG DS 301, Section 7.3.2)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NmtCommandData {
    /// No data payload (for plain state commands).
    None,
    /// Payload for NMTNetHostNameSet (Spec 7.3.2.1.1).
    HostName(String),
    /// Payload for NMTFlushArpEntry (Spec 7.3.2.1.2).
    FlushArp(NodeId),
}

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
    /// A map of multiplexed cycle number (1-based) -> NMT Info Service to publish.
    /// (Reference: OD 0x1F9E)
    pub publish_config: BTreeMap<u8, ServiceId>,
    pub current_multiplex_cycle: u8,
    pub node_info: BTreeMap<NodeId, CnInfo>,
    pub mandatory_nodes: Vec<NodeId>,
    pub isochronous_nodes: Vec<NodeId>,
    pub async_only_nodes: Vec<NodeId>,
    /// A dynamic cache mapping a CN's IP address to its discovered MAC address.
    /// This is populated from IdentResponse frames.
    pub arp_cache: BTreeMap<IpAddress, MacAddress>,
    pub next_isoch_node_idx: usize,
    pub current_phase: CyclePhase,
    pub current_polled_cn: Option<NodeId>,
    pub async_request_queue: BinaryHeap<AsyncRequest>,
    /// A high-priority queue for sending StatusRequests to CNs that need an ER flag.
    pub pending_er_requests: Vec<NodeId>,
    pub pending_status_requests: Vec<NodeId>,
    /// Queue for NMT commands (State and Managing) to be sent by the MN.
    /// (Command Type, Target Node ID, Command-specific Data)
    pub pending_nmt_commands: Vec<(MnNmtCommandRequest, NodeId, NmtCommandData)>,
    pub mn_async_send_queue: Vec<PowerlinkFrame>,
    /// Manages all stateful SDO client (outgoing) connections.
    pub sdo_client_manager: SdoClientManager,
    pub last_ident_poll_node_id: NodeId,
    pub last_status_poll_node_id: NodeId,
    pub next_tick_us: Option<u64>,
    pub pending_timeout_event: Option<DllMsEvent>,
    pub current_cycle_start_time_us: u64,
    pub initial_operational_actions_done: bool,
}

impl<'s> PdoHandler<'s> for MnContext<'s> {
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

/// A strongly-typed cache for a CN's identity information.
/// This data is read from an IdentResponse frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CnIdentity {
    pub device_type: u32,
    pub vendor_id: u32,
    pub product_code: u32,
    pub revision_no: u32,
    pub serial_no: u32,
}

/// A struct holding all state information for a single CN, as tracked by the MN.
// We can now re-add Copy and Eq because CnIdentity is Copy and Eq.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CnInfo {
    /// The high-level boot-up state of the CN.
    pub state: CnState,
    /// The last known NMT state reported by the CN in a PRes/StatusResponse.
    pub nmt_state: NmtState,
    /// The last known EN (Exception New) flag received from the CN.
    pub en_flag: bool,
    /// The last EA (Exception Acknowledge) flag sent *to* the CN by the MN.
    pub ea_flag: bool,
    /// Flag indicating the `CHECK_COMMUNICATION` step has passed.
    pub communication_ok: bool,
    /// Timestamp of the last successful PRes reception.
    pub last_pres_time_us: u64,
    /// Number of consecutive DLL errors (e.g., PRes timeouts).
    pub dll_errors: u32,
    /// Cached identity data read from IdentResponse.
    pub identity: Option<CnIdentity>,
    /// Current SDO state for this CN.
    pub sdo_state: SdoState,
}

impl Default for CnInfo {
    fn default() -> Self {
        Self {
            state: CnState::Unknown,
            nmt_state: NmtState::NmtNotActive,
            en_flag: false,
            ea_flag: false,
            communication_ok: false,
            last_pres_time_us: 0,
            dll_errors: 0,
            identity: None, // Starts as None
            sdo_state: SdoState::Idle,
        }
    }
}

/// Tracks the current SDO state for a CN, as tracked by the MN.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd)]
pub enum SdoState {
    Idle,
    InProgress,
    Done,
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