use super::payload;
use super::scheduler;
use crate::common::{NetTime, RelativeTime};
use crate::frame::basic::MacAddress;
use crate::frame::{
    ASndFrame, DllMsEvent, DllMsStateMachine, PResFrame, PowerlinkFrame, ServiceId, SocFrame,
    deserialize_frame,
    error::{
        DllError, DllErrorManager, ErrorCounters, ErrorHandler, LoggingErrorHandler,
        MnErrorCounters, NmtAction,
    },
};
use crate::frame::codec::CodecHelpers;
use crate::nmt::events::{NmtCommand, NmtEvent};
use crate::nmt::mn_state_machine::MnNmtStateMachine;
use crate::nmt::state_machine::NmtStateMachine;
use crate::nmt::states::NmtState;
use crate::node::{Node, NodeAction, PdoHandler};
use crate::od::{Object, ObjectDictionary, ObjectValue};
use crate::types::{C_ADR_BROADCAST_NODE_ID, C_ADR_MN_DEF_NODE_ID, NodeId};
use crate::PowerlinkError;
use alloc::collections::{BTreeMap, BinaryHeap};
use alloc::vec; // ERROR FIX: Added vec import
use alloc::vec::Vec;
use core::cmp::Ordering; // For BinaryHeap ordering
use log::{debug, error, info, trace, warn};

// Constants for OD access
const OD_IDX_NODE_ASSIGNMENT: u16 = 0x1F81;
const OD_IDX_CYCLE_TIME: u16 = 0x1006;
const OD_IDX_MN_PRES_TIMEOUT_LIST: u16 = 0x1F92;
const OD_IDX_CYCLE_TIMING_REC: u16 = 0x1F98; // NMT_CycleTiming_REC (for multiplex cycle len)
const OD_SUBIDX_MULTIPLEX_CYCLE_LEN: u8 = 7; // MultiplCycleCnt_U8 in 0x1F98
const OD_IDX_MULTIPLEX_ASSIGN: u16 = 0x1F9B; // NMT_MultiplCycleAssign_AU8
const OD_SUBIDX_ASYNC_SLOT_TIMEOUT: u8 = 2; // For use in this module

/// Internal state tracking for each configured CN.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd)]
pub(super) enum CnState {
    // Made pub(super)
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

/// Tracks the current phase within the POWERLINK cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CyclePhase {
    // Made pub(super)
    Idle,            // Waiting for next cycle start or PreOp1 SoA
    SoCSent,         // SoC has been sent, start isochronous phase
    IsochronousPReq, // PReq sent, waiting for PRes or timeout
    IsochronousDone, // All isochronous nodes polled
    AsynchronousSoA, // SoA sent, maybe waiting for ASnd or timeout
    AwaitingMnAsyncSend, // SoA sent to self, waiting to send ASnd(NMT)
}

/// Represents a pending asynchronous transmission request from a CN.
#[derive(Debug, Clone, Copy, Eq)]
pub(super) struct AsyncRequest {
    // Made pub(super)
    pub(super) node_id: NodeId,
    pub(super) priority: u8, // Higher value = higher priority (7 = NMT)
                             // Add timestamp or sequence for FIFO within priority? For now, no.
}

// Implement Ord and PartialOrd for AsyncRequest to use it in BinaryHeap (Max Heap)
impl Ord for AsyncRequest {
    fn cmp(&self, other: &Self) -> Ordering {
        self.priority.cmp(&other.priority) // Compare priorities directly
                                           // Add secondary comparison (e.g., timestamp) if needed for stable ordering
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
        // Useful for potentially removing specific requests, though BinaryHeap doesn't support easy removal.
        self.priority == other.priority && self.node_id == other.node_id
    }
}

/// Represents a complete POWERLINK Managing Node (MN).
pub struct ManagingNode<'s> {
    pub od: ObjectDictionary<'s>,
    pub(super) nmt_state_machine: MnNmtStateMachine,
    dll_state_machine: DllMsStateMachine,
    dll_error_manager: DllErrorManager<MnErrorCounters, LoggingErrorHandler>,
    pub(super) mac_address: MacAddress, // Made pub(super)
    cycle_time_us: u64,
    pub(super) multiplex_cycle_len: u8, // Length of multiplexed cycle (0 if disabled), pub(super)
    pub(super) multiplex_assign: BTreeMap<NodeId, u8>, // Map Node ID to its assigned multiplex cycle number (0=continuous), pub(super)
    current_multiplex_cycle: u8, // Counter for current multiplexed cycle (0 to multiplex_cycle_len - 1)
    pub(super) node_states: BTreeMap<NodeId, CnState>, // NodeId -> Current tracked state
    pub(super) mandatory_nodes: Vec<NodeId>, // List of mandatory Node IDs
    /// List of Node IDs for isochronous polling, read from OD 0x1F81/0x1F9C
    pub(super) isochronous_nodes: Vec<NodeId>, // Made pub(super)
    /// List of Node IDs configured as async-only.
    pub(super) async_only_nodes: Vec<NodeId>, // NEW
    /// Index into `isochronous_nodes` for the next node to poll in the *current* cycle.
    pub(super) next_isoch_node_idx: usize, // Made pub(super)
    /// Track the current phase within the cycle.
    pub(super) current_phase: CyclePhase, // Made pub(super)
    /// The NodeID of the CN currently being polled (if any).
    current_polled_cn: Option<NodeId>,
    /// Priority queue for pending asynchronous requests from CNs. Max heap based on priority.
    pub(super) async_request_queue: BinaryHeap<AsyncRequest>, // Changed to BinaryHeap
    /// Queue for NMT commands the MN needs to send (e.g., from error handling).
    pub(super) pending_nmt_commands: Vec<(NmtCommand, NodeId)>,
    /// Queue for generic async frames the MN application wants to send.
    pub(super) mn_async_send_queue: Vec<PowerlinkFrame>,
    pub(super) last_ident_poll_node_id: NodeId,
    pub(super) last_status_poll_node_id: NodeId, // NEW
    /// The absolute time in microseconds for the next scheduled tick (cycle start or timeout).
    next_tick_us: Option<u64>,
    /// Stores the event associated with a scheduled timeout.
    pending_timeout_event: Option<DllMsEvent>,
    /// Timestamp of the start of the current or last cycle (microseconds).
    current_cycle_start_time_us: u64,
    /// Flag to ensure one-time actions upon entering Operational are performed.
    initial_operational_actions_done: bool,
}

impl<'s> ManagingNode<'s> {
    /// Creates a new Managing Node.
    ///
    /// The application is responsible for creating and populating the Object Dictionary
    /// with all network configuration (e.g., 0x1F81 NodeAssignment)
    /// before passing it to this constructor.
    pub fn new(
        mut od: ObjectDictionary<'s>,
        mac_address: MacAddress,
    ) -> Result<Self, PowerlinkError> {
        info!("Creating new Managing Node.");
        // Initialise the OD, which involves loading from storage or applying defaults.
        od.init()?;

        // Validate that the user-provided OD contains all mandatory objects.
        od.validate_mandatory_objects(true)?; // true for MN validation

        // The NMT state machine's constructor is fallible
        let nmt_state_machine = MnNmtStateMachine::from_od(&od)?;

        // Read cycle time
        let cycle_time_us = od.read_u32(OD_IDX_CYCLE_TIME, 0).unwrap_or(0) as u64;
        if cycle_time_us == 0 {
            warn!("NMT_CycleLen_U32 (0x1006) is 0. MN will not start cyclic operation.");
        }

        // Read multiplex cycle length (from NMT_CycleTiming_REC)
        let multiplex_cycle_len =
            od.read_u8(OD_IDX_CYCLE_TIMING_REC, OD_SUBIDX_MULTIPLEX_CYCLE_LEN)
                .unwrap_or(0);
        if multiplex_cycle_len > 0 {
            info!(
                "Multiplexed cycle enabled with length: {}",
                multiplex_cycle_len
            );
        }

        // Read node assignment list (0x1F81) to build local state tracker
        // And build the initial isochronous node list and multiplex assignment map
        let mut node_states = BTreeMap::new();
        let mut mandatory_nodes = Vec::new();
        let mut isochronous_nodes = Vec::new();
        let mut async_only_nodes = Vec::new(); // NEW
        let mut multiplex_assign = BTreeMap::new();

        if let Some(Object::Array(entries)) = od.read_object(OD_IDX_NODE_ASSIGNMENT) {
            // Index 0 is NumberOfEntries, so skip it.
            for (i, entry) in entries.iter().enumerate().skip(1) {
                if let ObjectValue::Unsigned32(assignment) = entry {
                    // Bit 0: Node exists
                    if (assignment & 1) != 0 {
                        // Ensure Node ID is valid before using it
                        if let Ok(node_id) = NodeId::try_from(i as u8) {
                            node_states.insert(node_id, CnState::Unknown);
                            // Bit 3: Node is mandatory
                            if (assignment & (1 << 3)) != 0 {
                                mandatory_nodes.push(node_id);
                            }
                            // Bit 8: 0 = Isochronous, 1 = AsyncOnly
                            if (assignment & (1 << 8)) == 0 {
                                isochronous_nodes.push(node_id);

                                // Read multiplex assignment for this node (default to 0 = continuous)
                                let mux_cycle_no =
                                    od.read_u8(OD_IDX_MULTIPLEX_ASSIGN, node_id.0).unwrap_or(0);
                                if mux_cycle_no > 0 && multiplex_cycle_len == 0 {
                                    warn!("Node {} assigned to multiplex cycle {}, but multiplex cycle length is 0. Treating as continuous.", node_id.0, mux_cycle_no);
                                    multiplex_assign.insert(node_id, 0);
                                } else if mux_cycle_no > 0 && mux_cycle_no > multiplex_cycle_len {
                                    warn!("Node {} assigned to multiplex cycle {} which is > cycle length {}. Treating as continuous.", node_id.0, mux_cycle_no, multiplex_cycle_len);
                                    multiplex_assign.insert(node_id, 0);
                                } else {
                                    if mux_cycle_no > 0 {
                                        debug!(
                                            "Node {} assigned to multiplex cycle {}",
                                            node_id.0, mux_cycle_no
                                        );
                                    }
                                    multiplex_assign.insert(node_id, mux_cycle_no);
                                }
                            } else {
                                // NEW: This is an async-only node
                                async_only_nodes.push(node_id);
                            }
                        } else {
                            warn!("Invalid Node ID {} found in OD 0x1F81, skipping.", i);
                        }
                    }
                }
            }
        }
        // TODO: Optionally sort `isochronous_nodes` based on OD 0x1F9C

        info!(
            "MN configured to manage {} nodes ({} mandatory, {} isochronous, {} async-only). Multiplex Cycle Length: {}",
            node_states.len(),
            mandatory_nodes.len(),
            isochronous_nodes.len(),
            async_only_nodes.len(),
            multiplex_cycle_len
        );

        let mut node = Self {
            od,
            nmt_state_machine,
            dll_state_machine: DllMsStateMachine::new(),
            dll_error_manager: DllErrorManager::new(MnErrorCounters::new(), LoggingErrorHandler),
            mac_address,
            cycle_time_us,
            multiplex_cycle_len,
            multiplex_assign,
            current_multiplex_cycle: 0, // Start at cycle 0
            node_states,
            mandatory_nodes,
            isochronous_nodes,
            async_only_nodes, // NEW
            next_isoch_node_idx: 0,
            current_phase: CyclePhase::Idle,
            current_polled_cn: None,
            async_request_queue: BinaryHeap::new(), // Use BinaryHeap
            pending_nmt_commands: Vec::new(),
            mn_async_send_queue: Vec::new(),
            last_ident_poll_node_id: NodeId(0), // Use NodeId(0) as initial invalid value
            last_status_poll_node_id: NodeId(0), // NEW
            next_tick_us: None,                 // Initialize to None
            pending_timeout_event: None,
            current_cycle_start_time_us: 0,
            initial_operational_actions_done: false,
        };

        // Run the initial state transitions to get to NmtNotActive.
        node.nmt_state_machine
            .run_internal_initialisation(&mut node.od);

        Ok(node)
    }

    /// Queues a generic asynchronous frame to be sent by the MN.
    /// The MN will schedule this frame to be sent in the next available
    /// asynchronous slot according to its scheduling priorities.
    ///
    /// # Arguments
    /// * `frame` - The `PowerlinkFrame` to send. This should typically be an `ASnd` frame.
    pub fn queue_mn_async_frame(&mut self, frame: PowerlinkFrame) {
        info!("[MN] Queuing MN-initiated async frame: {:?}", frame);
        self.mn_async_send_queue.push(frame);
    }

    /// Internal function to process a deserialized `PowerlinkFrame`.
    /// The MN primarily *consumes* PRes and ASnd frames.
    fn process_frame(&mut self, frame: PowerlinkFrame, current_time_us: u64) -> NodeAction {
        // 1. Update NMT state machine based on the frame type.
        // NMT events from received frames don't typically change MN state directly,
        // but we might need this hook later.
        if let Some(event) = frame.nmt_event() {
            if self.nmt_state() != NmtState::NmtNotActive {
                self.nmt_state_machine.process_event(event, &mut self.od);
            }
        }

        // 2. Pass event to DLL state machine and handle errors.
        self.handle_dll_event(frame.dll_mn_event(), &frame);

        // 3. Handle specific frames
        match frame {
            PowerlinkFrame::PRes(pres_frame) => {
                // Update CN state based on reported NMT state
                self.update_cn_state(pres_frame.source, pres_frame.nmt_state);

                // Check if this PRes corresponds to the node we polled
                if self.current_phase == CyclePhase::IsochronousPReq
                    && self.current_polled_cn == Some(pres_frame.source)
                {
                    trace!(
                        "[MN] Received expected PRes from Node {}",
                        pres_frame.source.0
                    );
                    // Cancel pending PRes timeout
                    self.pending_timeout_event = None;
                    // Handle PDO consumption from PRes frames using the PdoHandler trait method.
                    self.consume_pdo_payload(
                        pres_frame.source,
                        &pres_frame.payload,
                        pres_frame.pdo_version,
                        pres_frame.flags.rd,
                    );
                    // Handle async requests flagged in PRes
                    self.handle_pres_flags(&pres_frame);
                    // PRes received, poll next CN or end isochronous phase
                    return self.advance_cycle_phase(current_time_us);
                } else {
                    warn!(
                        "[MN] Received unexpected PRes from Node {} (expected {:?}). Ignoring payload, checking flags.",
                        pres_frame.source.0, self.current_polled_cn
                    );
                    // Still handle async requests even if unexpected (cross-traffic scenario)
                    self.handle_pres_flags(&pres_frame);
                }
            }
            PowerlinkFrame::ASnd(asnd_frame) => {
                // Check if this ASnd corresponds to a granted async slot
                if self.current_phase == CyclePhase::AsynchronousSoA {
                    // TODO: Check if asnd_frame.source matches the node granted the slot
                    trace!(
                        "[MN] Received ASnd from Node {} during Async phase.",
                        asnd_frame.source.0
                    );
                    // Cancel pending ASnd timeout
                    self.pending_timeout_event = None;
                    // Handle asynchronous responses from CNs
                    self.handle_asnd_frame(&asnd_frame);
                    // ASnd received, the async phase for this cycle is done.
                    self.current_phase = CyclePhase::Idle;
                    // Schedule next cycle start (handled by main tick loop)
                } else {
                    // Could be an SDO response if MN is acting as SDO client,
                    // or Ident/StatusResponse during PreOp1 reduced cycle.
                    self.handle_asnd_frame(&asnd_frame);
                }
            }
            _ => {
                // MN ignores SoC, PReq (which it sent), and SoA (which it sent)
                // unless it's for state transitions (handled by NMT/DLL already).
            }
        }

        // Default action is NoAction if the frame didn't trigger a direct response need.
        NodeAction::NoAction
    }

    /// Passes an event to the DLL state machine and processes any resulting errors.
    fn handle_dll_event(&mut self, event: DllMsEvent, frame_context: &PowerlinkFrame) {
        // Determine destination node ID for error reporting, if applicable
        let reporting_node_id = match frame_context {
            PowerlinkFrame::PRes(f) => f.source,
            PowerlinkFrame::ASnd(f) => f.source,
            _ => self.current_polled_cn.unwrap_or(NodeId(0)), // Use polled CN if available for timeouts
        };

        // Determine context flags for DLL state machine transitions
        let response_expected = matches!(event, DllMsEvent::Pres | DllMsEvent::Asnd); // Simplified
        let async_in = !self.async_request_queue.is_empty(); // Simplified
        let async_out = false; // TODO: Track MN's own async requests
                               // Check remaining nodes *for the current multiplex cycle*
        let isochr_nodes_remaining =
            scheduler::has_more_isochronous_nodes(self, self.current_multiplex_cycle);
        let isochr = isochr_nodes_remaining || self.current_phase == CyclePhase::IsochronousPReq;
        let isochr_out = false; // TODO: Track MN PRes feature flag

        if let Some(errors) = self.dll_state_machine.process_event(
            event,
            self.nmt_state(),
            response_expected,
            async_in,
            async_out,
            isochr,
            isochr_out,
            reporting_node_id,
        ) {
            for error in errors {
                warn!("MN DLL state machine reported error: {:?}", error);
                // Attach correct node ID for per-node errors before handling
                let error_with_node = match error {
                    DllError::LossOfPres { .. } => DllError::LossOfPres {
                        node_id: reporting_node_id,
                    },
                    DllError::LatePres { .. } => DllError::LatePres {
                        node_id: reporting_node_id,
                    },
                    DllError::LossOfStatusRes { .. } => DllError::LossOfStatusRes {
                        node_id: reporting_node_id,
                    },
                    _ => error, // Keep original error if not per-node
                };

                let (nmt_action, _status_changed) =
                    self.dll_error_manager.handle_error(error_with_node);

                match nmt_action {
                    NmtAction::ResetNode(node_id) => {
                        warn!(
                            "[MN] DLL Error threshold met for Node {}. Requesting Node Reset.",
                            node_id.0
                        );
                        if let Some(state) = self.node_states.get_mut(&node_id) {
                            *state = CnState::Missing; // Mark node as missing
                        }
                        // Queue NMTResetNode command for this CN
                        self.pending_nmt_commands
                            .push((NmtCommand::ResetNode, node_id));
                    }
                    NmtAction::ResetCommunication => {
                        warn!("[MN] DLL Error threshold met. Requesting Communication Reset.");
                        self.nmt_state_machine
                            .process_event(NmtEvent::Error, &mut self.od);
                        // Reset cycle state after NMT reset
                        self.current_phase = CyclePhase::Idle;
                        self.pending_timeout_event = None;
                        self.next_tick_us = None; // Stop scheduling
                    }
                    NmtAction::None => {}
                }
            }
        }
    }

    /// Handles incoming ASnd frames, such as IdentResponse or NMTRequest
    fn handle_asnd_frame(&mut self, frame: &ASndFrame) {
        match frame.service_id {
            ServiceId::IdentResponse => {
                let node_id = frame.source;
                if let Some(state) = self.node_states.get_mut(&node_id) {
                    if *state == CnState::Unknown {
                        *state = CnState::Identified;
                        info!("[MN] Node {} identified.", node_id.0);
                        // After identifying a node, check if ready for next NMT state
                        scheduler::check_bootup_state(self);
                    } else {
                        // Node already identified, could be a response to a periodic check
                        trace!(
                            "[MN] Received subsequent IdentResponse from Node {}.",
                            node_id.0
                        );
                        // Potentially update MAC address mapping here if needed
                    }
                    // Update state based on NMTState field in IdentResponse payload
                    if frame.payload.len() > 2 {
                        if let Ok(nmt_state) = NmtState::try_from(frame.payload[2]) {
                            self.update_cn_state(node_id, nmt_state);
                        }
                    }
                } else {
                    warn!(
                        "[MN] Received IdentResponse from unconfigured Node {}.",
                        node_id.0
                    );
                }
            }
            ServiceId::StatusResponse => {
                let node_id = frame.source;
                // Update state based on NMTState field in StatusResponse payload
                if frame.payload.len() > 2 {
                    if let Ok(nmt_state) = NmtState::try_from(frame.payload[2]) {
                        self.update_cn_state(node_id, nmt_state);
                    }
                } else {
                    warn!(
                        "[MN] Received StatusResponse from Node {} with invalid payload length.",
                        node_id.0
                    );
                }
                // TODO: Process error flags (EN, EC) from StatusResponse payload (needs payload parsing)
                trace!(
                    "[MN] Received StatusResponse from CN {}. Full processing not yet implemented.",
                    frame.source.0
                );
            }
            ServiceId::NmtRequest => {
                // TODO: Parse NMT request payload and queue it for MN execution
                warn!(
                    "[MN] NMTRequest from CN {} not yet supported.",
                    frame.source.0
                );
            }
            ServiceId::Sdo => {
                // TODO: Handle SDO (which for MN is usually a client response)
                trace!(
                    "[MN] Received SDO ASnd from CN {}. SDO Client functionality not yet implemented.",
                    frame.source.0
                );
            }
            ServiceId::NmtCommand => {
                // Added missing arm
                warn!(
                    "[MN] Received unexpected NMT Command via ASnd from Node {}. Ignoring.",
                    frame.source.0
                );
            }
        }
    }

    // `consume_pres_payload` method is removed. The trait implementation is used directly.

    /// Checks the flags in a received PRes frame and queues async requests.
    fn handle_pres_flags(&mut self, pres: &PResFrame) {
        let rs_count = pres.flags.rs.get();
        if rs_count > 0 {
            let priority = pres.flags.pr as u8;
            debug!(
                "[MN] Node {} requesting async transmission (RS={}, PR={})",
                pres.source.0, rs_count, priority
            );
            // Simple queuing: push one request per PRes flag, ignore RS count > 1
            // Use BinaryHeap which automatically handles priority.
            // Avoid adding duplicate requests? BinaryHeap makes this hard to check efficiently.
            // Let the scheduler handle potential duplicates if needed.
            self.async_request_queue.push(AsyncRequest {
                node_id: pres.source,
                priority,
            });
        }
    }

    /// Updates the MN's internal state tracker for a CN based on its reported NMT state.
    fn update_cn_state(&mut self, node_id: NodeId, reported_state: NmtState) {
        if let Some(current_state_ref) = self.node_states.get_mut(&node_id) {
            // Map reported NMT state to internal CnState enum
            let new_state = match reported_state {
                NmtState::NmtPreOperational1 => CnState::Identified, // Can receive PRes/Status in PreOp1
                NmtState::NmtPreOperational2 | NmtState::NmtReadyToOperate => {
                    CnState::PreOperational
                }
                NmtState::NmtOperational => CnState::Operational,
                NmtState::NmtCsStopped => CnState::Stopped,
                // If CN reports a reset state, mark as Unknown until identified again
                NmtState::NmtGsInitialising
                | NmtState::NmtGsResetApplication
                | NmtState::NmtGsResetCommunication
                | NmtState::NmtGsResetConfiguration => CnState::Unknown,
                // Keep current state if reported state is unexpected or non-CN state
                _ => *current_state_ref,
            };

            if *current_state_ref != new_state {
                info!(
                    "[MN] Node {} state changed: {:?} -> {:?}",
                    node_id.0, *current_state_ref, new_state
                );
                *current_state_ref = new_state;
                // After state update, check if MN NMT state can transition
                scheduler::check_bootup_state(self); // Check if NMT state can advance
            }
        }
    }

    /// Determines the next action in the isochronous phase (send next PReq or SoA).
    fn advance_cycle_phase(&mut self, current_time_us: u64) -> NodeAction {
        // Find the next active node to poll using the helper function, passing current multiplex cycle
        if let Some(node_id) =
            scheduler::get_next_isochronous_node_to_poll(self, self.current_multiplex_cycle)
        {
            // Found the next node
            self.current_polled_cn = Some(node_id);
            self.current_phase = CyclePhase::IsochronousPReq;

            // Trigger PReq event for DLL state machine (implicit via sending PReq)
            // self.handle_dll_event(DllMsEvent::SocTrig, ...); // Incorrect event here

            // Schedule timeout for PRes
            let timeout_ns = self
                .od
                .read_u32(OD_IDX_MN_PRES_TIMEOUT_LIST, node_id.0)
                .unwrap_or(25000) as u64; // Default 25us in ns
            self.schedule_timeout(
                current_time_us + (timeout_ns / 1000),
                DllMsEvent::PresTimeout,
            );

            // Determine if the target node is multiplexed for the MS flag
            let is_multiplexed = self.multiplex_assign.get(&node_id).copied().unwrap_or(0) > 0;
            // ERROR FIX: build_preq_frame now takes &self. No mutable borrow conflict.
            let frame = payload::build_preq_frame(self, node_id, is_multiplexed);
            return self.serialize_and_prepare_action(frame);
        }

        // No more isochronous nodes to poll, end of isochronous phase
        debug!(
            "[MN] Isochronous phase complete for cycle {}.",
            self.current_multiplex_cycle
        );
        self.current_polled_cn = None;
        self.current_phase = CyclePhase::IsochronousDone;

        // Let the scheduler decide the next async action (Ident, Status, or CN request)
        let (req_service, target_node) = scheduler::determine_next_async_action(self);
        
        // If the target is a CN, set timeout.
        if target_node.0 != C_ADR_MN_DEF_NODE_ID && req_service != crate::frame::RequestedServiceId::NoService {
            self.current_phase = CyclePhase::AsynchronousSoA;
            let timeout_ns = self.od.read_u32(OD_IDX_CYCLE_TIMING_REC, OD_SUBIDX_ASYNC_SLOT_TIMEOUT).unwrap_or(100_000) as u64;
            self.schedule_timeout(current_time_us + (timeout_ns / 1000), DllMsEvent::AsndTimeout);
        } else if target_node.0 == C_ADR_MN_DEF_NODE_ID {
             // If the target is the MN itself, update the cycle phase.
            self.current_phase = CyclePhase::AwaitingMnAsyncSend;
        } else {
            self.current_phase = CyclePhase::Idle;
        }

        let frame = payload::build_soa_frame(
            self,
            req_service,
            target_node,
        );

        self.serialize_and_prepare_action(frame)
    }

    /// Helper to potentially schedule a DLL timeout event.
    pub(super) fn schedule_timeout(&mut self, deadline_us: u64, event: DllMsEvent) {
        // Schedule the timeout. If it's earlier than the next cycle start,
        // it becomes the next tick time.
        let next_event_time = if let Some(next_cycle) = self.next_tick_us {
            deadline_us.min(next_cycle)
        } else {
            deadline_us // No cycle start scheduled, timeout is the only event
        };

        // If the new event time is earlier than the currently scheduled tick OR no tick is scheduled
        if self.next_tick_us.is_none() || next_event_time < self.next_tick_us.unwrap() {
            self.next_tick_us = Some(next_event_time);
            // Store the event associated ONLY if it's the timeout deadline we just set
            if next_event_time == deadline_us {
                // Only store if no other event is already pending for this exact time
                if self.pending_timeout_event.is_none()
                    || self.next_tick_us.unwrap() != deadline_us
                {
                    self.pending_timeout_event = Some(event);
                    debug!("[MN] Scheduled {:?} timeout at {}us", event, deadline_us);
                } else {
                    warn!("[MN] Could not schedule {:?} timeout at {}us, another event {:?} already pending for same time.", event, deadline_us, self.pending_timeout_event.unwrap());
                }
            } else {
                // The next cycle start is sooner, clear any pending timeout event
                self.pending_timeout_event = None;
            }
        // If the timeout deadline matches the *existing* next_tick_us (which might be the cycle start),
        // store the timeout event, as it should be processed *before* the cycle start logic in tick().
        } else if next_event_time == deadline_us && self.next_tick_us.is_some() {
            // Only overwrite if no other timeout is already pending for the same time
            if self.pending_timeout_event.is_none() {
                self.pending_timeout_event = Some(event);
                debug!(
                    "[MN] Scheduled {:?} timeout coinciding with next cycle start at {}us",
                    event, deadline_us
                );
            } else {
                warn!("[MN] Could not schedule {:?} timeout at {}us, another event {:?} already pending for same time.", event, deadline_us, self.pending_timeout_event.unwrap());
            }
        } else {
            debug!(
                "[MN] Timeout {:?} at {}us is later than next scheduled event at {}us. Ignoring schedule.",
                event,
                deadline_us,
                self.next_tick_us.unwrap_or(0)
            );
        }
    }

    /// Gets CN MAC address from OD 0x1F84. Made pub(super).
    pub(super) fn get_cn_mac_address(&self, node_id: NodeId) -> Option<MacAddress> {
        // Read object 0x1F84 NMT_MNDeviceTypeIdList_AU32 (assuming it holds MAC temporarily)
        const OD_IDX_MAC_MAP: u16 = 0x1F84; // Using DeviceType list index as placeholder
        if let Some(Object::Array(entries)) = self.od.read_object(OD_IDX_MAC_MAP) {
            // OD Array sub-index = Node ID. Index 0 = count.
            // Ensure sub-index access is within bounds of the actual array length
            if (node_id.0 as usize) < entries.len() {
                // Assuming the U32 holds MAC address bytes (needs proper OD object)
                if let Some(ObjectValue::Unsigned32(mac_val_u32)) =
                    entries.get(node_id.0 as usize)
                {
                    let mac_bytes = mac_val_u32.to_le_bytes(); // Assuming LE storage in U32
                                                              // Use only first 6 bytes if stored in U32
                    if mac_bytes[0..6].iter().any(|&b| b != 0) {
                        // Check if not all zero
                        return Some(MacAddress(mac_bytes[0..6].try_into().unwrap()));
                    } else {
                        trace!(
                            "[MN] Zero MAC entry found for Node {} in OD {:#06X}.",
                            node_id.0,
                            OD_IDX_MAC_MAP
                        );
                    }
                } else {
                    trace!(
                        "[MN] No MAC entry (or wrong type) found for Node {} in OD {:#06X}.",
                        node_id.0,
                        OD_IDX_MAC_MAP
                    );
                }
            } else {
                trace!(
                    "[MN] Node ID {} out of bounds for MAC map OD {:#06X} (len {}).",
                    node_id.0,
                    OD_IDX_MAC_MAP,
                    entries.len()
                );
            }
        } else {
            trace!(
                "[MN] OD object {:#06X} (MAC map placeholder) not found or not an array.",
                OD_IDX_MAC_MAP
            );
        }
        None // Not found or invalid
    }

    /// Helper to serialize a PowerlinkFrame and prepare the NodeAction.
    fn serialize_and_prepare_action(&self, frame: PowerlinkFrame) -> NodeAction {
        let mut buf = vec![0u8; 1500];
        // Serialize Eth header first
        let eth_header = match &frame {
            PowerlinkFrame::Soc(f) => &f.eth_header,
            PowerlinkFrame::PReq(f) => &f.eth_header,
            PowerlinkFrame::PRes(f) => &f.eth_header,
            PowerlinkFrame::SoA(f) => &f.eth_header,
            PowerlinkFrame::ASnd(f) => &f.eth_header,
        };
        CodecHelpers::serialize_eth_header(eth_header, &mut buf);
        // Then serialize PL part
        match frame.serialize(&mut buf[14..]) {
            Ok(pl_size) => {
                let total_size = 14 + pl_size;
                buf.truncate(total_size);
                trace!("[MN] Sending frame ({} bytes): {:02X?}", total_size, &buf);
                NodeAction::SendFrame(buf)
            }
            Err(e) => {
                error!("Failed to serialize response frame: {:?}", e);
                NodeAction::NoAction
            }
        }
    }
}

// Implement the PdoHandler trait for ManagingNode
impl<'s> PdoHandler<'s> for ManagingNode<'s> {
    fn od(&mut self) -> &mut ObjectDictionary<'s> {
        &mut self.od
    }

    // Match the trait signature using `impl Trait`
    fn dll_error_manager(&mut self) -> &mut DllErrorManager<impl ErrorCounters, impl ErrorHandler> {
        &mut self.dll_error_manager
    }
}

impl<'s> Node for ManagingNode<'s> {
    /// Processes a raw byte buffer received from the network at a specific time.
    fn process_raw_frame(&mut self, buffer: &[u8], current_time_us: u64) -> NodeAction {
        // MN must check for other MNs when in NotActive
        if self.nmt_state() == NmtState::NmtNotActive
            && buffer.len() >= 14 // Check length before slicing
            && buffer[12..14] == crate::types::C_DLL_ETHERTYPE_EPL.to_be_bytes()
        {
            // Check if the source MAC is different from our own
            if buffer.len() >= 12 && buffer[6..12] != self.mac_address.0 {
                warn!(
                    "[MN] POWERLINK frame detected while in NotActive state from different MAC {:02X?}. Another MN may be present.",
                    &buffer[6..12]
                );
                // Log DLL error
                let (_, _) = self.dll_error_manager.handle_error(DllError::MultipleMn);
                // NMT state machine will handle this error (e.g., stay in NotActive)
                // We still try to deserialize to check frame type for DLL state machine context.
            } else {
                trace!("[MN] Ignoring received frame from self in NotActive state.");
                return NodeAction::NoAction;
            }
        }

        // --- Deserialize Frame ---
        // Pass the *full* buffer (including Eth header) to the new deserialize_frame
        match deserialize_frame(buffer) {
            Ok(frame) => {
                // deserialize_frame now returns a frame with the correct Eth header
                self.process_frame(frame, current_time_us)
            }
            Err(PowerlinkError::InvalidEthernetFrame) => {
                trace!("Ignoring non-POWERLINK frame (wrong EtherType).");
                NodeAction::NoAction
            }
            // BufferTooShort can happen if eth header itself is truncated
            Err(PowerlinkError::BufferTooShort) => {
                warn!("[MN] Received truncated Ethernet frame.");
                let (_, _) = self.dll_error_manager.handle_error(DllError::InvalidFormat); // Treat as invalid format
                NodeAction::NoAction
            }
            Err(PowerlinkError::InvalidPlFrame) | Err(PowerlinkError::InvalidMessageType(_)) => {
                // This error is now more reliable since deserialize_frame checks EtherType first
                warn!(
                    "[MN] Could not deserialize POWERLINK frame (correct EtherType): {:?}. Buffer: {:02X?}",
                    buffer, buffer
                );
                let (_, _) = self.dll_error_manager.handle_error(DllError::InvalidFormat);
                NodeAction::NoAction
            }
            Err(e) => {
                // Handle other potential errors from deserialize_frame
                warn!(
                    "[MN] Error during frame deserialization: {:?}. Buffer: {:02X?}",
                    e, buffer
                );
                let (_, _) = self.dll_error_manager.handle_error(DllError::InvalidFormat); // Treat others as invalid format too
                NodeAction::NoAction
            }
        }
    }

    /// The MN's tick is its primary scheduler.
    fn tick(&mut self, current_time_us: u64) -> NodeAction {
        // --- 1. Handle one-time actions on entering Operational ---
        // This is a special, high-priority action that runs once.
        if self.nmt_state() == NmtState::NmtOperational && !self.initial_operational_actions_done {
            self.initial_operational_actions_done = true;
            // NMT_StartUp_U32.Bit1 determines broadcast (1) or individual (0) start
            if (self.nmt_state_machine.startup_flags & (1 << 1)) != 0 {
                info!("[MN] Sending NMTStartNode (Broadcast) to all CNs.");
                return self.serialize_and_prepare_action(payload::build_nmt_command_frame(
                    self,
                    NmtCommand::StartNode,
                    NodeId(C_ADR_BROADCAST_NODE_ID),
                ));
            } else {
                info!("[MN] Sending NMTStartNode (Unicast) to individual CNs.");
                // We just queue the first command; the scheduler will handle sending them.
                if let Some(&node_id) = self.mandatory_nodes.first() {
                    self.pending_nmt_commands.push((NmtCommand::StartNode, node_id));
                    // The async scheduler will pick this up at the next SoA.
                }
            }
        } else if self.nmt_state() < NmtState::NmtOperational {
            // Reset the flag if we leave the operational state.
            self.initial_operational_actions_done = false;
        }

        // --- 2. Handle immediate, non-time-based follow-up actions ---
        // These actions must happen immediately after a previous action,
        // ignoring the main cycle deadline.
        match self.current_phase {
            CyclePhase::AwaitingMnAsyncSend => {
                // We just sent an SoA to ourselves. Now send the queued frame.
                // Priority: NMT commands first, then generic MN requests.
                if let Some((command, target_node)) = self.pending_nmt_commands.pop() {
                    info!(
                        "[MN] Sending queued NMT command {:?} for Node {} in async phase.",
                        command, target_node.0
                    );
                    self.current_phase = CyclePhase::Idle; // Consume this async phase
                    return self.serialize_and_prepare_action(payload::build_nmt_command_frame(
                        self,
                        command,
                        target_node,
                    ));
                } else if let Some(frame) = self.mn_async_send_queue.pop() {
                    info!("[MN] Sending queued generic async frame in async phase.");
                    self.current_phase = CyclePhase::Idle;
                    return self.serialize_and_prepare_action(frame);
                } else {
                    warn!(
                        "[MN] Was in AwaitingMnAsyncSend, but all send queues are empty. Resetting phase."
                    );
                    self.current_phase = CyclePhase::Idle;
                    // Fall through to time-based checks
                }
            }
            CyclePhase::SoCSent => {
                // We just sent an SoC. Immediately send the first PReq (or SoA).
                // This call will set its own timeouts and phase.
                return self.advance_cycle_phase(current_time_us);
            }
            _ => {
                // Not an immediate follow-up state.
                // Proceed to time-based checks below.
            }
        }

        // --- 3. Handle time-based actions (Timeouts and Cycle Start) ---

        // Handle the very first tick in NotActive to set the initial timeout.
        if self.nmt_state() == NmtState::NmtNotActive && self.next_tick_us.is_none() {
            let timeout_us = self.nmt_state_machine.wait_not_active_timeout as u64;
            if timeout_us > 0 {
                self.next_tick_us = Some(current_time_us + timeout_us);
                debug!(
                    "[MN] Scheduling NotActive timeout check at {}us",
                    self.next_tick_us.unwrap()
                );
            }
            return NodeAction::NoAction;
        }

        // Check if a scheduled deadline has passed.
        let deadline = self.next_tick_us.unwrap_or(0);
        let deadline_passed = current_time_us >= deadline;

        if !deadline_passed {
            return NodeAction::NoAction; // Not time for any timed action yet.
        }

        // A deadline has been met.
        let mut action = NodeAction::NoAction;
        let mut schedule_next_cycle = true; // Assume we will schedule the next cycle start

        if let Some(timeout_event) = self.pending_timeout_event.take() {
            // --- A. Handle a specific DLL Timeout (PRes/ASnd) ---
            let missed_node = self.current_polled_cn.unwrap_or(NodeId(0));
            warn!(
                "[MN] Timeout event {:?} occurred at {}us (expected Node: {})",
                timeout_event, current_time_us, missed_node.0
            );

            // Handle the timeout event
            let dummy_frame = PowerlinkFrame::Soc(SocFrame::new(
                self.mac_address,
                Default::default(),
                NetTime { seconds: 0, nanoseconds: 0 },
                RelativeTime { seconds: 0, nanoseconds: 0 },
            ));
            self.handle_dll_event(timeout_event, &dummy_frame);

            if timeout_event == DllMsEvent::PresTimeout {
                if let Some(state) = self.node_states.get_mut(&missed_node) {
                    if *state >= CnState::Identified && *state != CnState::Stopped {
                        *state = CnState::Missing;
                    }
                }
                // After timeout, immediately schedule the next isochronous action or SoA
                action = self.advance_cycle_phase(current_time_us);
                schedule_next_cycle = false;
            } else if timeout_event == DllMsEvent::AsndTimeout {
                warn!("[MN] ASnd timeout occurred for Node {}", missed_node.0);
                self.current_phase = CyclePhase::Idle;
                schedule_next_cycle = true;
            }
        } else if self.nmt_state() == NmtState::NmtNotActive {
            // --- B. Handle the NotActive Timeout ---
            info!("[MN] NotActive timeout expired. Proceeding to boot.");
            self.nmt_state_machine
                .process_event(NmtEvent::Timeout, &mut self.od);
            // The NMT state is now PreOp1. The logic will fall through to block 4.
            schedule_next_cycle = true;
        }

        // --- 4. Handle Cycle Start (if no other action was taken) ---
        // This block runs if:
        // - No timeout occurred (or an AsndTimeout occurred, which just resets the phase to Idle).
        // - The state is Idle (or we just transitioned to PreOp1).
        // - The deadline has passed.
        let nmt_state = self.nmt_state(); // Re-check state in case NotActive timed out
        if action == NodeAction::NoAction && self.current_phase == CyclePhase::Idle {
            self.current_cycle_start_time_us = current_time_us;
            debug!(
                "[MN] Tick: Cycle start at {}us (State: {:?})",
                current_time_us, nmt_state
            );

            if nmt_state >= NmtState::NmtPreOperational2 && self.multiplex_cycle_len > 0 {
                self.current_multiplex_cycle =
                    (self.current_multiplex_cycle + 1) % self.multiplex_cycle_len;
            }
            if nmt_state >= NmtState::NmtPreOperational1 {
                self.dll_error_manager.on_cycle_complete();
            }

            action = match nmt_state {
                NmtState::NmtPreOperational1 => {
                    // Start of a "Reduced Cycle" (just async phase)
                    // This will find an IdentRequest to send.
                    self.advance_cycle_phase(current_time_us)
                }
                NmtState::NmtOperational
                | NmtState::NmtReadyToOperate
                | NmtState::NmtPreOperational2 => {
                    // Start of a new Isochronous Cycle
                    self.current_phase = CyclePhase::SoCSent;
                    self.next_isoch_node_idx = 0;
                    self.current_polled_cn = None;
                    self.pending_timeout_event = None;
                    self.serialize_and_prepare_action(payload::build_soc_frame(
                        self,
                        self.current_multiplex_cycle,
                        self.multiplex_cycle_len,
                    ))
                }
                _ => NodeAction::NoAction, // No cyclic actions in other states
            };

            // `advance_cycle_phase` or `SoCSent` will schedule their own follow-ups.
            schedule_next_cycle = false;
        }

        // --- 5. Schedule the next cycle tick ---
        // This logic runs if:
        // - A timeout (like AsndTimeout) occurred, and we need to schedule the next main cycle.
        // - The NotActive timeout just expired, and we need to schedule the first PreOp1 action.
        // - We were in Idle, but the NMT state was not one that starts a cycle (e.g., NotActive).
        if schedule_next_cycle {
            self.cycle_time_us = self.od.read_u32(OD_IDX_CYCLE_TIME, 0).unwrap_or(0) as u64;
            if self.cycle_time_us > 0 && nmt_state >= NmtState::NmtPreOperational1 {
                let base_time = if self.current_cycle_start_time_us > 0
                    && current_time_us
                        < self.current_cycle_start_time_us + self.cycle_time_us
                {
                    self.current_cycle_start_time_us
                } else {
                    current_time_us
                };
                let next_cycle_start = (base_time / self.cycle_time_us + 1) * self.cycle_time_us;

                if self.pending_timeout_event.is_none()
                    || next_cycle_start <= self.next_tick_us.unwrap_or(u64::MAX)
                {
                    self.next_tick_us = Some(next_cycle_start);
                    if self.pending_timeout_event.is_some() && next_cycle_start <= deadline {
                        self.pending_timeout_event = None;
                    }
                    debug!("[MN] Scheduling next cycle start at {}us", next_cycle_start);
                } else {
                    // A timeout is already scheduled and it's sooner than the next cycle start.
                    debug!(
                        "[MN] Next cycle start {}us deferred due to pending timeout at {}us",
                        next_cycle_start,
                        self.next_tick_us.unwrap_or(0)
                    );
                }
            }
        }

        action
    }

    fn nmt_state(&self) -> NmtState {
        self.nmt_state_machine.current_state()
    }

    fn next_action_time(&self) -> Option<u64> {
        // If an immediate follow-up action is required, signal to call tick() again straight away.
        // We use the current cycle start time as the "deadline" which has just passed.
        if matches!(
            self.current_phase,
            CyclePhase::SoCSent | CyclePhase::AwaitingMnAsyncSend
        ) {
            return Some(self.current_cycle_start_time_us);
        }

        if self.nmt_state() == NmtState::NmtNotActive && self.next_tick_us.is_none() {
            return Some(0); // Signal to call tick immediately to set the first NotActive timeout.
        }
        self.next_tick_us
    }
}