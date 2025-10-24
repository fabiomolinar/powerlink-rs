// crates/powerlink-rs/src/node/mn/main.rs

use super::scheduler;
use crate::common::{NetTime, RelativeTime};
use crate::frame::basic::MacAddress;
use crate::frame::control::{SoAFlags, SocFlags};
use crate::frame::{
    deserialize_frame,
    error::{DllError, DllErrorManager, ErrorCounters, ErrorHandler, LoggingErrorHandler, MnErrorCounters},
    ASndFrame, DllMsStateMachine, PowerlinkFrame, PResFrame, RequestedServiceId, ServiceId,
    SoAFrame, SocFrame, Codec,
};
use crate::nmt::mn_state_machine::MnNmtStateMachine;
use crate::nmt::state_machine::NmtStateMachine;
use crate::nmt::states::NmtState;
use crate::nmt::events::NmtEvent;
use crate::node::{Node, NodeAction, PdoHandler};
use crate::od::{Object, ObjectDictionary, ObjectValue};
use crate::types::{EPLVersion, NodeId};
use crate::PowerlinkError;
use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;
use log::{debug, error, info, trace, warn};

// Constants for OD access
const OD_IDX_NODE_ASSIGNMENT: u16 = 0x1F81;
const OD_IDX_CYCLE_TIME: u16 = 0x1006;

/// Internal state tracking for each configured CN.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CnState {
    /// Initial state, node is configured but not heard from.
    Unknown,
    /// Node has responded to IdentRequest.
    Identified,
    /// Node is in PreOp2 or ReadyToOperate.
    PreOperational,
    /// Node is in Operational.
    Operational,
    /// Node is stopped.
    Stopped,
}

/// Represents a complete POWERLINK Managing Node (MN).
pub struct ManagingNode<'s> {
    pub od: ObjectDictionary<'s>,
    pub(super) nmt_state_machine: MnNmtStateMachine,
    dll_state_machine: DllMsStateMachine,
    dll_error_manager: DllErrorManager<MnErrorCounters, LoggingErrorHandler>,
    mac_address: MacAddress,
    cycle_time_us: u64,
    pub(super) node_states: BTreeMap<NodeId, CnState>,
    pub(super) mandatory_nodes: Vec<NodeId>,
    pub(super) last_ident_poll_node_id: NodeId,
    /// The absolute time in microseconds for the next scheduled tick.
    next_tick_us: Option<u64>,
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
        let mut node_states = BTreeMap::new();
        let mut mandatory_nodes = Vec::new();
        if let Some(Object::Array(entries)) = od.read_object(OD_IDX_NODE_ASSIGNMENT) {
            // Index 0 is NumberOfEntries, so skip it.
            for (i, entry) in entries.iter().enumerate().skip(1) {
                if let ObjectValue::Unsigned32(assignment) = entry {
                    // Bit 0: Node exists
                    if (assignment & 1) != 0 {
                        let node_id = NodeId(i as u8);
                        node_states.insert(node_id, CnState::Unknown);
                        // Bit 3: Node is mandatory
                        if (assignment & (1 << 3)) != 0 {
                            mandatory_nodes.push(node_id);
                        }
                    }
                }
            }
        }
        info!(
            "MN configured to manage {} nodes ({} mandatory).",
            node_states.len(),
            mandatory_nodes.len()
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
            last_ident_poll_node_id: NodeId(0),
            next_tick_us: None, // Initialize to None
        };

        // Run the initial state transitions to get to NmtNotActive.
        node.nmt_state_machine
            .run_internal_initialisation(&mut node.od);
        
        Ok(node)
    }

    /// Internal function to process a deserialized `PowerlinkFrame`.
    /// The MN primarily *consumes* PRes and ASnd frames.
    fn process_frame(&mut self, frame: PowerlinkFrame, _current_time_us: u64) -> NodeAction {
        // 1. Update NMT state machine based on the frame type.
        if let Some(event) = frame.nmt_event() {
             // Don't transition NMT state based on received frames if MN is not yet active
             if self.nmt_state() != NmtState::NmtNotActive {
                 self.nmt_state_machine.process_event(event, &mut self.od);
             }
        }

        // 2. Update DLL state machine.
        // TODO: This logic needs to be fully implemented.
        if let Some(errors) = self.dll_state_machine.process_event(
            frame.dll_mn_event(),
            self.nmt_state(),
            false, /* response_expected */
            false, /* async_in */
            false, /* async_out */
            false, /* isochr */
            false, /* isochr_out */
            NodeId(0), /* placeholder dest_node_id */
        ) {
            for error in errors {
                warn!("MN DLL state machine reported error: {:?}", error);
                let nmt_action = self.dll_error_manager.handle_error(error);
                // TODO: Handle NmtAction::ResetNode(id)
                if nmt_action != crate::frame::NmtAction::None {
                     self.nmt_state_machine
                         .process_event(NmtEvent::Error, &mut self.od);
                 }
            }
        }

        // 3. Handle specific frames
        match frame {
            PowerlinkFrame::PRes(pres_frame) => {
                // Handle PDO consumption from PRes frames.
                self.consume_pres_payload(&pres_frame);
            }
            PowerlinkFrame::ASnd(asnd_frame) => {
                // Handle asynchronous responses from CNs
                self.handle_asnd_frame(&asnd_frame);
            }
            _ => {
                // MN ignores SoC, PReq (which it sent), and SoA (which it sent)
            }
        }

        // The MN's response logic is driven by its internal `tick` (scheduler),
        // not directly by processing incoming frames.
        NodeAction::NoAction
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
             _ => { // Handle other Service IDs if necessary, or just trace
                 trace!("[MN] Ignoring ASnd with unhandled ServiceID {:?} from Node {}", frame.service_id, frame.source.0);
             }
        }
    }

    /// Reads RPDO mappings for a given source CN and writes
    /// data from the PRes payload into the MN's Object Dictionary.
    fn consume_pres_payload(&mut self, pres: &PResFrame) {
        // Delegate to the PdoHandler trait's default implementation
        self.consume_pdo_payload(pres.source, &pres.payload, pres.pdo_version, pres.flags.rd);
    }

    /// Builds and serializes a SoC frame.
    fn build_soc_frame(&self) -> NodeAction {
        // TODO: Get real NetTime and RelativeTime from system clock or PTP
        let net_time = NetTime {
            seconds: 0,
            nanoseconds: 0,
        };
        let relative_time = RelativeTime {
            seconds: 0,
            nanoseconds: 0,
        };
        // TODO: Determine SoC flags (mc, ps) based on current cycle state
        let soc_flags = SocFlags::default();

        let soc_frame = SocFrame::new(self.mac_address, soc_flags, net_time, relative_time);

        let mut buf = vec![0u8; 64]; // SoC is min frame size
        match soc_frame.serialize(&mut buf) {
            Ok(size) => {
                buf.truncate(size.max(60)); // Ensure min Ethernet frame size
                NodeAction::SendFrame(buf)
            }
            Err(e) => {
                error!("[MN] Failed to serialize SoC frame: {:?}", e);
                NodeAction::NoAction
            }
        }
    }

    /// Builds and serializes an SoA(IdentRequest) frame.
    fn build_soa_ident_request(&self, target_node_id: NodeId) -> NodeAction {
        debug!(
            "[MN] Building SoA(IdentRequest) for Node {}",
            target_node_id.0
        );
        // TODO: Read actual EPLVersion from OD (0x1F83)
        let epl_version = EPLVersion(self.od.read_u8(0x1F83, 0).unwrap_or(0x15)); // Default to 1.5 if not found

        let req_service = if target_node_id.0 == 0 {
            RequestedServiceId::NoService // 0 indicates no specific target
        } else {
            RequestedServiceId::IdentRequest
        };

        let soa_frame = SoAFrame::new(
            self.mac_address,
            self.nmt_state(),
            SoAFlags::default(),
            req_service,
            target_node_id,
            epl_version,
        );
        let mut buf = vec![0u8; 64]; // SoA is min frame size
        match soa_frame.serialize(&mut buf) {
            Ok(size) => {
                buf.truncate(size.max(60)); // Ensure min Ethernet frame size
                NodeAction::SendFrame(buf)
            }
            Err(e) => {
                error!(
                    "[MN] Failed to serialize SoA(IdentRequest) frame: {:?}",
                    e
                );
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
    fn dll_error_manager(
        &mut self,
    ) -> &mut DllErrorManager<impl ErrorCounters, impl ErrorHandler> {
        &mut self.dll_error_manager
    }
}

impl<'s> Node for ManagingNode<'s> {
    /// Processes a raw byte buffer received from the network at a specific time.
    fn process_raw_frame(&mut self, buffer: &[u8], current_time_us: u64) -> NodeAction {
        // MN must check for other MNs when in NotActive
        if self.nmt_state() == NmtState::NmtNotActive
            && buffer.len() > 14
            && buffer[12..14] == crate::types::C_DLL_ETHERTYPE_EPL.to_be_bytes()
        {
            warn!(
                "[MN] POWERLINK frame detected while in NotActive state. Another MN may be present."
            );
            // Log DLL error
            let _ = self.dll_error_manager.handle_error(DllError::MultipleMn);
            // NMT state machine will handle this error (e.g., stay in NotActive)
            // We still process the frame to get the NMT event (if any)
        }

        match deserialize_frame(buffer) {
            Ok(frame) => self.process_frame(frame, current_time_us),
            Err(PowerlinkError::InvalidEthernetFrame) => {
                trace!("Ignoring non-POWERLINK frame (wrong EtherType).");
                NodeAction::NoAction
            }
            Err(e) => {
                warn!("[MN] Could not deserialize frame: {:?}", e);
                let _ = self.dll_error_manager.handle_error(DllError::InvalidFormat);
                NodeAction::NoAction
            }
        }
    }

    /// The MN's tick is its primary scheduler.
    fn tick(&mut self, current_time_us: u64) -> NodeAction {
        let deadline_passed = self.next_tick_us.map_or(true, |d| current_time_us >= d);
        if !deadline_passed {
            return NodeAction::NoAction;
        }

        let mut action = NodeAction::NoAction;
        
        // --- NotActive Timeout Check ---
        if self.nmt_state() == NmtState::NmtNotActive {
            info!("[MN] NotActive timeout expired. No other MN detected. Proceeding to boot.");
            self.nmt_state_machine
                .process_event(NmtEvent::Timeout, &mut self.od);
            // After transitioning, immediately schedule the next action for the new state.
            let cycle_time_us = self.od.read_u32(OD_IDX_CYCLE_TIME, 0).unwrap_or(0) as u64;
            self.next_tick_us = Some(current_time_us + cycle_time_us);
            // Fall through to execute the first action of PreOp1 immediately.
        }

        // Re-evaluate state in case it changed
        let nmt_state = self.nmt_state();

        debug!(
            "[MN] Tick: Cycle/Action start at {}us (State: {:?})",
            current_time_us, nmt_state
        );
        
        if nmt_state != NmtState::NmtNotActive {
             self.dll_error_manager.on_cycle_complete();
        }

        action = match nmt_state {
            NmtState::NmtPreOperational1 => {
                if let Some(node_to_poll) = scheduler::find_next_node_to_identify(self) {
                    self.build_soa_ident_request(node_to_poll)
                } else {
                    self.build_soa_ident_request(NodeId(0))
                }
            }
            NmtState::NmtOperational
            | NmtState::NmtReadyToOperate
            | NmtState::NmtPreOperational2 => {
                self.build_soc_frame()
            }
            _ => NodeAction::NoAction,
        };

        // Schedule the next tick
        self.cycle_time_us = self.od.read_u32(OD_IDX_CYCLE_TIME, 0).unwrap_or(0) as u64;
        if self.cycle_time_us > 0 {
             self.next_tick_us = Some(current_time_us + self.cycle_time_us);
        } else {
             self.next_tick_us = None; // Stop scheduling if cycle time is 0
        }
        
        action
    }

    fn nmt_state(&self) -> NmtState {
        self.nmt_state_machine.current_state()
    }

    fn next_action_time(&self) -> Option<u64> {
        // If we are in NotActive for the first time, schedule the initial check.
        if self.nmt_state() == NmtState::NmtNotActive && self.next_tick_us.is_none() {
             let timeout_ns = self.nmt_state_machine.wait_not_active_timeout;
             // We need current_time to set an absolute deadline.
             // This indicates to the user's loop that it should call tick() once immediately
             // to get the first deadline scheduled.
             return Some(0);
        }
        self.next_tick_us
    }
}