// crates/powerlink-rs/src/node/mn/main.rs
use super::payload; // Use the new payload module
use super::scheduler;
use crate::common::{NetTime, RelativeTime}; // Added imports
use crate::PowerlinkError;
use crate::frame::basic::MacAddress; // Keep MacAddress import here
use crate::frame::{
    ASndFrame, DllMsEvent, DllMsStateMachine, PResFrame, PowerlinkFrame, ServiceId, SocFrame,
    // deserialize_frame is now used in process_raw_frame
    deserialize_frame,
    error::{
        DllError, DllErrorManager, ErrorCounters, ErrorHandler, LoggingErrorHandler,
        MnErrorCounters, NmtAction,
    },
};
use crate::nmt::events::NmtEvent;
use crate::nmt::mn_state_machine::MnNmtStateMachine;
use crate::nmt::state_machine::NmtStateMachine;
use crate::nmt::states::NmtState;
use crate::node::{Node, NodeAction, PdoHandler};
use crate::od::{Object, ObjectDictionary, ObjectValue};
use crate::types::NodeId;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::vec::Vec; // Keep Vec import
use log::{debug, info, trace, warn}; // Removed unused 'error'

// Constants for OD access
const OD_IDX_NODE_ASSIGNMENT: u16 = 0x1F81;
const OD_IDX_CYCLE_TIME: u16 = 0x1006;
// Moved TPDO/PReq constants to payload.rs
const OD_IDX_MN_PRES_TIMEOUT_LIST: u16 = 0x1F92; // Keep timeout list index

/// Internal state tracking for each configured CN.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd)]
pub(super) enum CnState { // Made pub(super)
    /// Initial state, node is configured but not heard from.
    Unknown,
    /// Node has responded to IdentRequest.
    Identified,
    /// Node is in PreOp2 or ReadyToOperate.
    PreOperational, // Keep variant, even if warned as unused for now
    /// Node is in Operational.
    Operational, // Keep variant, even if warned as unused for now
    /// Node is stopped.
    Stopped,
    /// Node missed a PRes or timed out.
    Missing,
}

/// Tracks the current phase within the POWERLINK cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CyclePhase { // Made pub(super)
    Idle,            // Waiting for next cycle start
    SoCSent,         // SoC has been sent, start isochronous phase
    IsochronousPReq, // PReq sent, waiting for PRes or timeout
    IsochronousDone, // All isochronous nodes polled
    AsynchronousSoA, // SoA sent, maybe waiting for ASnd or timeout
}

/// Represents a pending asynchronous transmission request from a CN.
#[derive(Debug, Clone, Copy)]
pub(super) struct AsyncRequest { // Made pub(super)
    pub(super) node_id: NodeId,
    pub(super) priority: u8,
}

/// Represents a complete POWERLINK Managing Node (MN).
pub struct ManagingNode<'s> {
    pub od: ObjectDictionary<'s>,
    pub(super) nmt_state_machine: MnNmtStateMachine,
    dll_state_machine: DllMsStateMachine,
    dll_error_manager: DllErrorManager<MnErrorCounters, LoggingErrorHandler>,
    pub(super) mac_address: MacAddress, // Made pub(super)
    cycle_time_us: u64,
    pub(super) node_states: BTreeMap<NodeId, CnState>,
    pub(super) mandatory_nodes: Vec<NodeId>,
    /// List of Node IDs for isochronous polling, read from OD 0x1F81/0x1F9C
    pub(super) isochronous_nodes: Vec<NodeId>, // Made pub(super)
    /// Index into `isochronous_nodes` for the next node to poll.
    pub(super) next_isoch_node_idx: usize, // Made pub(super)
    /// Track the current phase within the cycle.
    pub(super) current_phase: CyclePhase, // Made pub(super)
    /// The NodeID of the CN currently being polled (if any).
    current_polled_cn: Option<NodeId>,
    /// Simple queue for pending asynchronous requests from CNs.
    pub(super) async_request_queue: VecDeque<AsyncRequest>, // Made pub(super)
    pub(super) last_ident_poll_node_id: NodeId,
    /// The absolute time in microseconds for the next scheduled tick (cycle start or timeout).
    next_tick_us: Option<u64>,
    /// Stores the event associated with a scheduled timeout.
    pending_timeout_event: Option<DllMsEvent>,
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

        // Read node assignment list (0x1F81) to build local state tracker
        // And build the initial isochronous node list
        let mut node_states = BTreeMap::new();
        let mut mandatory_nodes = Vec::new();
        let mut isochronous_nodes = Vec::new();
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
            "MN configured to manage {} nodes ({} mandatory, {} isochronous).",
            node_states.len(),
            mandatory_nodes.len(),
            isochronous_nodes.len()
        );

        let mut node = Self {
            od,
            nmt_state_machine,
            dll_state_machine: DllMsStateMachine::new(),
            dll_error_manager: DllErrorManager::new(MnErrorCounters::new(), LoggingErrorHandler),
            mac_address,
            cycle_time_us,
            node_states,
            mandatory_nodes,
            isochronous_nodes,
            next_isoch_node_idx: 0,
            current_phase: CyclePhase::Idle,
            current_polled_cn: None,
            async_request_queue: VecDeque::new(),
            last_ident_poll_node_id: NodeId(0), // Use NodeId(0) as initial invalid value
            next_tick_us: None, // Initialize to None
            pending_timeout_event: None,
        };

        // Run the initial state transitions to get to NmtNotActive.
        node.nmt_state_machine
            .run_internal_initialisation(&mut node.od);

        Ok(node)
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
                    return self.schedule_next_isochronous_action(current_time_us);
                } else {
                    warn!(
                        "[MN] Received unexpected PRes from Node {} (expected {:?}). Ignoring.",
                        pres_frame.source.0, self.current_polled_cn
                    );
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
                    // Could be an SDO response if MN is acting as SDO client
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
        let isochr_nodes_remaining = self.next_isoch_node_idx < self.isochronous_nodes.len();
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

                let nmt_action = self.dll_error_manager.handle_error(error_with_node);

                match nmt_action {
                    NmtAction::ResetNode(node_id) => {
                        warn!(
                            "[MN] DLL Error threshold met for Node {}. Requesting Node Reset.",
                            node_id.0
                        );
                        if let Some(state) = self.node_states.get_mut(&node_id) {
                            *state = CnState::Missing; // Mark node as missing
                        }
                        // TODO: Queue NMTResetNode command for this CN
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
                        // After identifying a node, check if all mandatory nodes are up
                        scheduler::check_bootup_state(self);
                    }
                } else {
                    warn!(
                        "[MN] Received IdentResponse from unconfigured Node {}.",
                        node_id.0
                    );
                }
            }
            ServiceId::StatusResponse => {
                // TODO: Update NMT state of the CN in our tracker based on NMTState field in StatusResponse
                // This requires parsing the StatusResponse payload.
                trace!(
                    "[MN] Received StatusResponse from CN {}. Processing not yet implemented.",
                    frame.source.0
                );
            }
            ServiceId::NmtRequest => {
                // TODO: Handle NMT request from CN and queue it in the async scheduler
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
            ServiceId::NmtCommand => { // Added missing arm
                 warn!("[MN] Received unexpected NMT Command via ASnd from Node {}. Ignoring.", frame.source.0);
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
            // Simple FIFO queuing for now, ignoring priority and RS count > 1
            if !self
                .async_request_queue
                .iter()
                .any(|req| req.node_id == pres.source)
            {
                self.async_request_queue.push_back(AsyncRequest {
                    node_id: pres.source,
                    priority,
                });
            }
        }
    }

    /// Determines the next action in the isochronous phase (send next PReq or SoA).
    fn schedule_next_isochronous_action(&mut self, current_time_us: u64) -> NodeAction {
        // Find the next active node to poll using the helper function
        if let Some(node_id) = scheduler::get_next_isochronous_node_to_poll(self) {
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

            return payload::build_preq_frame(self, node_id); // Use payload module
        }

        // No more isochronous nodes to poll, end of isochronous phase
        debug!("[MN] Isochronous phase complete.");
        self.current_polled_cn = None;
        self.current_phase = CyclePhase::IsochronousDone;
        // Pass PRes/PResTimeout event for the last PReq before proceeding to SoA
        // (Handled in process_frame or tick timeout logic)
        // Proceed to build and send SoA
        payload::build_soa_frame(self) // Use payload module
    }

    /// Helper to potentially schedule a DLL timeout event.
    fn schedule_timeout(&mut self, deadline_us: u64, event: DllMsEvent) {
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
                self.pending_timeout_event = Some(event);
                debug!("[MN] Scheduled {:?} timeout at {}us", event, deadline_us);
            } else {
                // The next cycle start is sooner, clear any pending timeout event
                self.pending_timeout_event = None;
            }
        // If the timeout deadline matches the *existing* next_tick_us (which might be the cycle start),
        // store the timeout event, as it should be processed *before* the cycle start logic in tick().
        } else if next_event_time == deadline_us && self.next_tick_us.is_some() {
            self.pending_timeout_event = Some(event);
            debug!(
                "[MN] Scheduled {:?} timeout coinciding with next cycle start at {}us",
                event, deadline_us
            );
        }
    }

    /// Gets CN MAC address from OD 0x1F84. Made pub(super).
    pub(super) fn get_cn_mac_address(&self, node_id: NodeId) -> Option<MacAddress> {
        if let Some(Object::Array(entries)) = self.od.read_object(0x1F84) {
            // OD Array sub-index = Node ID. Index 0 = count.
            if let Some(ObjectValue::OctetString(mac_bytes)) = entries.get(node_id.0 as usize) {
                // Check if MAC is valid (not all zeros and correct length)
                if mac_bytes.len() == 6 && mac_bytes.iter().any(|&b| b != 0) {
                    // Correct conversion from Vec<u8> to [u8; 6]
                    match mac_bytes.as_slice().try_into() {
                        Ok(arr) => return Some(MacAddress(arr)), // Wrap in MacAddress
                        Err(_) => {
                            // This should be impossible due to length check, but handle defensively
                            warn!(
                                "[MN] Failed to convert Vec<u8> to [u8; 6] for Node {}.",
                                node_id.0
                            );
                        }
                    }
                } else {
                    trace!(
                        "[MN] Invalid or zero MAC entry found for Node {} in OD 0x1F84.",
                        node_id.0
                    );
                }
            } else {
                trace!(
                    "[MN] No MAC entry found for Node {} in OD 0x1F84.",
                    node_id.0
                );
            }
        } else {
            trace!("[MN] OD object 0x1F84 (MAC map) not found or not an array.");
        }
        None // Not found or invalid
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
            warn!(
                "[MN] POWERLINK frame detected while in NotActive state. Another MN may be present."
            );
            // Log DLL error
            let _ = self.dll_error_manager.handle_error(DllError::MultipleMn);
            // NMT state machine will handle this error (e.g., stay in NotActive)
            // We still try to deserialize to check frame type for DLL state machine context.
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
            Err(PowerlinkError::InvalidPlFrame) | Err(PowerlinkError::InvalidMessageType(_)) => {
                 // This error is now more reliable since deserialize_frame checks EtherType first
                 warn!("[MN] Could not deserialize POWERLINK frame (correct EtherType): {:?}", buffer);
                 let _ = self.dll_error_manager.handle_error(DllError::InvalidFormat);
                 NodeAction::NoAction
            }
            Err(e) => { // Handle other potential errors from deserialize_frame
                warn!("[MN] Error during frame deserialization: {:?}", e);
                NodeAction::NoAction
            }
        }
    }


    /// The MN's tick is its primary scheduler.
    fn tick(&mut self, current_time_us: u64) -> NodeAction {
        let deadline = self.next_tick_us.unwrap_or(0);
        // Use `>=` for deadline check to handle exact matches
        let deadline_passed = current_time_us >= deadline;

        // Skip if not in NotActive and deadline hasn't passed
        if !deadline_passed && self.nmt_state() != NmtState::NmtNotActive {
            return NodeAction::NoAction;
        }

        let mut action = NodeAction::NoAction;
        let mut schedule_next_cycle = true; // Assume we will schedule the next cycle start

        // --- Handle Timeout Events ---
        // Process timeout ONLY if the deadline has passed AND there's a pending timeout event
        if deadline_passed && self.next_tick_us.is_some() {
            if let Some(timeout_event) = self.pending_timeout_event.take() {
                 // Check if the current time actually met or exceeded the specific timeout deadline
                 if current_time_us >= deadline {
                    let missed_node = self.current_polled_cn.unwrap_or(NodeId(0));
                    warn!(
                        "[MN] Timeout event {:?} occurred at {}us (expected Node: {})",
                        timeout_event, current_time_us, missed_node.0
                    );

                    // Handle the timeout event using the DLL state machine
                    // Create a dummy frame context for handle_dll_event
                    let dummy_frame = PowerlinkFrame::Soc(SocFrame::new(
                        self.mac_address,
                        Default::default(),
                        NetTime { // Use imported type
                            seconds: 0,
                            nanoseconds: 0,
                        },
                        RelativeTime { // Use imported type
                            seconds: 0,
                            nanoseconds: 0,
                        },
                    ));
                    self.handle_dll_event(timeout_event, &dummy_frame);

                     // Mark the node as missing if it was a PRes timeout
                    if timeout_event == DllMsEvent::PresTimeout {
                        if let Some(state) = self.node_states.get_mut(&missed_node) {
                           *state = CnState::Missing;
                       }
                        // After timeout, immediately schedule the next isochronous action or SoA
                       action = self.schedule_next_isochronous_action(current_time_us);
                        // Don't schedule the next main cycle yet, the scheduler handles timing now
                        schedule_next_cycle = false;
                    }
                    else if timeout_event == DllMsEvent::AsndTimeout {
                         warn!("[MN] ASnd timeout occurred for Node {}", missed_node.0);
                         self.current_phase = CyclePhase::Idle; // Async phase ended by timeout
                         // Don't schedule next cycle yet, let main logic do it below
                         schedule_next_cycle = true; // Let cycle scheduling happen below
                    }
                 } else {
                     // Tick called before timeout deadline, put the event back
                     self.pending_timeout_event = Some(timeout_event);
                     debug!("[MN] Tick called at {}us before timeout deadline {}us, deferring.", current_time_us, deadline);
                     return NodeAction::NoAction; // No action now
                 }
            }
        }


        // --- NotActive Timeout Check ---
        // Only run this if we are in NotActive AND the timeout specifically scheduled for it has passed.
        // Also check pending_timeout_event is None to ensure this isn't a different timeout event.
        if self.nmt_state() == NmtState::NmtNotActive && deadline_passed && self.pending_timeout_event.is_none() {
            info!("[MN] NotActive timeout expired. No other MN detected. Proceeding to boot.");
            self.nmt_state_machine
                .process_event(NmtEvent::Timeout, &mut self.od);
            // Fall through to execute the first action of PreOp1 immediately.
            // Clear the consumed timeout deadline.
            self.next_tick_us = None; // Reset deadline
            schedule_next_cycle = true; // Ensure cycle scheduling happens below
        }

        // Re-evaluate state in case it changed
        let nmt_state = self.nmt_state();

        // Only proceed with cycle logic if a specific action wasn't determined by timeout handling above
        // AND if the tick is for the start of the cycle (or first action in PreOp1)
        if action == NodeAction::NoAction
            && (self.current_phase == CyclePhase::Idle || nmt_state == NmtState::NmtPreOperational1)
        {
            debug!(
                "[MN] Tick: Cycle/Action start at {}us (State: {:?}, Phase: {:?})",
                current_time_us, nmt_state, self.current_phase
            );

            if nmt_state != NmtState::NmtNotActive && nmt_state != NmtState::NmtBasicEthernet {
                self.dll_error_manager.on_cycle_complete();
            }

            action = match nmt_state {
                NmtState::NmtPreOperational1 => {
                    // Poll for identification
                    if let Some(node_to_poll) = scheduler::find_next_node_to_identify(self) {
                        payload::build_soa_ident_request(self, node_to_poll) // Use payload module
                    } else {
                        // If all known nodes are identified, check if ready for PreOp2
                        scheduler::check_bootup_state(self);
                        payload::build_soa_ident_request(self, NodeId(0)) // Send SoA(NoService)
                    }
                }
                NmtState::NmtOperational
                | NmtState::NmtReadyToOperate
                | NmtState::NmtPreOperational2 => {
                    // Start of a new cycle
                    self.current_phase = CyclePhase::SoCSent;
                    self.next_isoch_node_idx = 0; // Reset for polling
                    self.current_polled_cn = None;
                    self.pending_timeout_event = None; // Clear any stale timeout event
                    payload::build_soc_frame(self) // Use payload module
                }
                _ => NodeAction::NoAction, // No cyclic actions in other states
            };

            // If we just sent SoC, immediately schedule the first PReq or SoA
            if self.current_phase == CyclePhase::SoCSent {
                action = self.schedule_next_isochronous_action(current_time_us);
                // Don't schedule next cycle start yet, let the PReq/PRes sequence or timeouts handle it
                schedule_next_cycle = false;
            } else if nmt_state == NmtState::NmtPreOperational1 {
                // In PreOp1, scheduling is simpler (Reduced Cycle) - schedule next SoA/Ident poll
                // Use cycle_time as a basic interval for now, real reduced cycle is faster
                schedule_next_cycle = true; // Let the logic below handle scheduling
                self.current_phase = CyclePhase::Idle; // Reset phase after action
            }
        }

        // Schedule the next main cycle tick only if not waiting for a specific timeout within the cycle
        if schedule_next_cycle {
            self.cycle_time_us = self.od.read_u32(OD_IDX_CYCLE_TIME, 0).unwrap_or(0) as u64;
             if self.cycle_time_us > 0
                && nmt_state != NmtState::NmtNotActive
                && nmt_state != NmtState::NmtBasicEthernet
            {
                // Calculate next cycle start based on current time and cycle time
                // Ensure alignment to the cycle grid
                let cycles_passed = current_time_us / self.cycle_time_us;
                let next_cycle_start = (cycles_passed + 1) * self.cycle_time_us;

                // Only update if this is sooner than any pending timeout, or if no timeout pending
                 // Or if the pending timeout deadline is the same as the cycle start (cycle start takes precedence)
                if self.pending_timeout_event.is_none()
                   || next_cycle_start <= deadline // Check <= deadline
                {
                    self.next_tick_us = Some(next_cycle_start);
                    // If cycle start takes precedence, clear the timeout event
                    if self.pending_timeout_event.is_some() && next_cycle_start <= deadline {
                         self.pending_timeout_event = None;
                    }
                    debug!("[MN] Scheduling next cycle start at {}us", next_cycle_start);
                } else {
                    debug!("[MN] Next cycle start {}us deferred due to pending timeout at {}us", next_cycle_start, deadline);
                    // Keep the existing (earlier) deadline for the timeout
                    self.next_tick_us = Some(deadline);
                }

            } else {
                // Stop scheduling if cycle time is 0 or not in a cyclic state
                // (Except for the initial NotActive timeout)
                if nmt_state != NmtState::NmtNotActive {
                    self.next_tick_us = None;
                    self.pending_timeout_event = None;
                } else if self.next_tick_us.is_none() { // If initial NotActive timeout hasn't been set
                     let timeout_us = self.nmt_state_machine.wait_not_active_timeout as u64;
                     if timeout_us > 0 {
                         self.next_tick_us = Some(current_time_us + timeout_us);
                         debug!("[MN] Scheduling NotActive timeout check at {}us", self.next_tick_us.unwrap());
                     }
                }
            }
        } else {
            debug!("[MN] Deferring next cycle scheduling due to intra-cycle action/timeout.");
        }

        action
    }


    fn nmt_state(&self) -> NmtState {
        self.nmt_state_machine.current_state()
    }

    fn next_action_time(&self) -> Option<u64> {
        // If we are in NotActive for the first time, schedule the initial check.
        if self.nmt_state() == NmtState::NmtNotActive && self.next_tick_us.is_none() {
            // Need current time to set an absolute deadline.
            // Return 0 to signal the user loop to call tick immediately to get the first real deadline.
            return Some(0);
        }
        self.next_tick_us
    }
}

