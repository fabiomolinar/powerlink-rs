// crates/powerlink-rs/src/node/mn/main.rs
use super::cycle;
use super::events;
use super::payload;
use super::state::{AsyncRequest, CnState, CyclePhase};
use crate::PowerlinkError;
use crate::common::{NetTime, RelativeTime};
use crate::frame::basic::MacAddress;
use crate::frame::codec::CodecHelpers;
use crate::frame::{
    DllMsEvent, DllMsStateMachine, PowerlinkFrame, SocFrame, deserialize_frame,
    error::{
        DllError, DllErrorManager, ErrorCounters, ErrorHandler, LoggingErrorHandler,
        MnErrorCounters,
    },
};
use crate::nmt::NmtStateMachine;
use crate::nmt::events::{NmtCommand, NmtEvent};
use crate::nmt::mn_state_machine::MnNmtStateMachine;
use crate::nmt::states::NmtState;
use crate::node::{Node, NodeAction, PdoHandler};
use crate::od::{Object, ObjectDictionary, ObjectValue};
use crate::types::{C_ADR_BROADCAST_NODE_ID, NodeId};
use alloc::collections::{BTreeMap, BinaryHeap};
use alloc::vec;
use alloc::vec::Vec;
use log::{debug, error, info, trace, warn};

// Constants for OD access used in this file.
const OD_IDX_NODE_ASSIGNMENT: u16 = 0x1F81;
const OD_IDX_CYCLE_TIME: u16 = 0x1006;
const OD_IDX_CYCLE_TIMING_REC: u16 = 0x1F98;
const OD_SUBIDX_MULTIPLEX_CYCLE_LEN: u8 = 7;
const OD_IDX_MULTIPLEX_ASSIGN: u16 = 0x1F9B;

/// Represents a complete POWERLINK Managing Node (MN).
pub struct ManagingNode<'s> {
    pub od: ObjectDictionary<'s>,
    pub(super) nmt_state_machine: MnNmtStateMachine,
    pub(super) dll_state_machine: DllMsStateMachine,
    pub(super) dll_error_manager: DllErrorManager<MnErrorCounters, LoggingErrorHandler>,
    pub(super) mac_address: MacAddress,
    pub(super) cycle_time_us: u64,
    pub(super) multiplex_cycle_len: u8,
    pub(super) multiplex_assign: BTreeMap<NodeId, u8>,
    pub(super) current_multiplex_cycle: u8,
    pub(super) node_states: BTreeMap<NodeId, CnState>,
    pub(super) mandatory_nodes: Vec<NodeId>,
    pub(super) isochronous_nodes: Vec<NodeId>,
    pub(super) async_only_nodes: Vec<NodeId>,
    pub(super) next_isoch_node_idx: usize,
    pub(super) current_phase: CyclePhase,
    pub(super) current_polled_cn: Option<NodeId>,
    pub(super) async_request_queue: BinaryHeap<AsyncRequest>,
    pub(super) pending_nmt_commands: Vec<(NmtCommand, NodeId)>,
    pub(super) mn_async_send_queue: Vec<PowerlinkFrame>,
    pub(super) last_ident_poll_node_id: NodeId,
    pub(super) last_status_poll_node_id: NodeId,
    pub(super) next_tick_us: Option<u64>,
    pub(super) pending_timeout_event: Option<DllMsEvent>,
    pub(super) current_cycle_start_time_us: u64,
    initial_operational_actions_done: bool,
}

impl<'s> ManagingNode<'s> {
    /// Creates a new Managing Node.
    pub fn new(
        mut od: ObjectDictionary<'s>,
        mac_address: MacAddress,
    ) -> Result<Self, PowerlinkError> {
        info!("Creating new Managing Node.");
        od.init()?;
        od.validate_mandatory_objects(true)?;

        let nmt_state_machine = MnNmtStateMachine::from_od(&od)?;
        let cycle_time_us = od.read_u32(OD_IDX_CYCLE_TIME, 0).unwrap_or(0) as u64;
        let multiplex_cycle_len = od
            .read_u8(OD_IDX_CYCLE_TIMING_REC, OD_SUBIDX_MULTIPLEX_CYCLE_LEN)
            .unwrap_or(0);

        let mut node_states = BTreeMap::new();
        let mut mandatory_nodes = Vec::new();
        let mut isochronous_nodes = Vec::new();
        let mut async_only_nodes = Vec::new();
        let mut multiplex_assign = BTreeMap::new();

        if let Some(Object::Array(entries)) = od.read_object(OD_IDX_NODE_ASSIGNMENT) {
            for (i, entry) in entries.iter().enumerate().skip(1) {
                if let ObjectValue::Unsigned32(assignment) = entry {
                    if (assignment & 1) != 0 {
                        if let Ok(node_id) = NodeId::try_from(i as u8) {
                            node_states.insert(node_id, CnState::Unknown);
                            if (assignment & (1 << 3)) != 0 {
                                mandatory_nodes.push(node_id);
                            }
                            if (assignment & (1 << 8)) == 0 {
                                isochronous_nodes.push(node_id);
                                let mux_cycle_no =
                                    od.read_u8(OD_IDX_MULTIPLEX_ASSIGN, node_id.0).unwrap_or(0);
                                multiplex_assign.insert(node_id, mux_cycle_no);
                            } else {
                                async_only_nodes.push(node_id);
                            }
                        }
                    }
                }
            }
        }
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
            current_multiplex_cycle: 0,
            node_states,
            mandatory_nodes,
            isochronous_nodes,
            async_only_nodes,
            next_isoch_node_idx: 0,
            current_phase: CyclePhase::Idle,
            current_polled_cn: None,
            async_request_queue: BinaryHeap::new(),
            pending_nmt_commands: Vec::new(),
            mn_async_send_queue: Vec::new(),
            last_ident_poll_node_id: NodeId(0),
            last_status_poll_node_id: NodeId(0),
            next_tick_us: None,
            pending_timeout_event: None,
            current_cycle_start_time_us: 0,
            initial_operational_actions_done: false,
        };
        node.nmt_state_machine
            .run_internal_initialisation(&mut node.od);
        Ok(node)
    }

    /// Queues a generic asynchronous frame to be sent by the MN.
    pub fn queue_mn_async_frame(&mut self, frame: PowerlinkFrame) {
        info!("[MN] Queuing MN-initiated async frame: {:?}", frame);
        self.mn_async_send_queue.push(frame);
    }

    /// Advances the POWERLINK cycle to the next phase (e.g., next PReq or SoA).
    pub(super) fn advance_cycle_phase(&mut self, current_time_us: u64) -> NodeAction {
        cycle::advance_cycle_phase(self, current_time_us)
    }

    /// Helper to potentially schedule a DLL timeout event.
    pub(super) fn schedule_timeout(&mut self, deadline_us: u64, event: DllMsEvent) {
        let next_event_time = self
            .next_tick_us
            .map_or(deadline_us, |next| deadline_us.min(next));
        if self.next_tick_us.is_none() || next_event_time < self.next_tick_us.unwrap() {
            self.next_tick_us = Some(next_event_time);
            if next_event_time == deadline_us {
                self.pending_timeout_event = Some(event);
                debug!("[MN] Scheduled {:?} timeout at {}us", event, deadline_us);
            } else {
                self.pending_timeout_event = None;
            }
        } else if next_event_time == deadline_us && self.pending_timeout_event.is_none() {
            self.pending_timeout_event = Some(event);
            debug!(
                "[MN] Scheduled {:?} timeout coinciding with next event at {}us",
                event, deadline_us
            );
        }
    }

    /// Gets a CN's MAC address from the Object Dictionary.
    pub(super) fn get_cn_mac_address(&self, node_id: NodeId) -> Option<MacAddress> {
        const OD_IDX_MAC_MAP: u16 = 0x1F84;
        if let Some(Object::Array(entries)) = self.od.read_object(OD_IDX_MAC_MAP) {
            if let Some(ObjectValue::Unsigned32(mac_val_u32)) = entries.get(node_id.0 as usize) {
                let mac_bytes = mac_val_u32.to_le_bytes();
                if mac_bytes[0..6].iter().any(|&b| b != 0) {
                    return Some(MacAddress(mac_bytes[0..6].try_into().unwrap()));
                }
            }
        }
        None
    }

    /// Helper to serialize a PowerlinkFrame and prepare the NodeAction.
    pub(super) fn serialize_and_prepare_action(&self, frame: PowerlinkFrame) -> NodeAction {
        let mut buf = vec![0u8; 1500];
        let eth_header = match &frame {
            PowerlinkFrame::Soc(f) => &f.eth_header,
            PowerlinkFrame::PReq(f) => &f.eth_header,
            PowerlinkFrame::SoA(f) => &f.eth_header,
            PowerlinkFrame::ASnd(f) => &f.eth_header,
            // PRes is not sent by MN
            PowerlinkFrame::PRes(_) => {
                error!("[MN] Attempted to serialize a PRes frame, which is invalid for an MN.");
                return NodeAction::NoAction;
            }
        };
        CodecHelpers::serialize_eth_header(eth_header, &mut buf);
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

impl<'s> PdoHandler<'s> for ManagingNode<'s> {
    fn od(&mut self) -> &mut ObjectDictionary<'s> {
        &mut self.od
    }

    fn dll_error_manager(&mut self) -> &mut DllErrorManager<impl ErrorCounters, impl ErrorHandler> {
        &mut self.dll_error_manager
    }
}

impl<'s> Node for ManagingNode<'s> {
    /// Processes a raw byte buffer received from the network.
    fn process_raw_frame(&mut self, buffer: &[u8], current_time_us: u64) -> NodeAction {
        if self.nmt_state() == NmtState::NmtNotActive
            && buffer.get(12..14) == Some(&crate::types::C_DLL_ETHERTYPE_EPL.to_be_bytes())
            && buffer.get(6..12) != Some(&self.mac_address.0)
        {
            warn!("[MN] POWERLINK frame detected while in NotActive state from another MN.");
            self.dll_error_manager.handle_error(DllError::MultipleMn);
        }

        match deserialize_frame(buffer) {
            Ok(frame) => {
                // The frame was successfully deserialized, pass it to the event handler.
                // The event handler will decide if an immediate action is needed, but process_raw_frame
                // itself does not return it. The main loop will call tick() again.
                events::process_frame(self, frame, current_time_us);
                NodeAction::NoAction
            }
            Err(e) if e != PowerlinkError::InvalidEthernetFrame => {
                // Log any POWERLINK-specific deserialization errors.
                warn!("[MN] Error during frame deserialization: {:?}", e);
                self.dll_error_manager.handle_error(DllError::InvalidFormat);
                NodeAction::NoAction
            }
            _ => {
                // Ignore non-POWERLINK frames silently.
                NodeAction::NoAction
            }
        }
    }

    /// The MN's main scheduler tick.
    fn tick(&mut self, current_time_us: u64) -> NodeAction {
        // --- 1. Handle one-time actions ---
        if self.nmt_state() == NmtState::NmtOperational && !self.initial_operational_actions_done {
            self.initial_operational_actions_done = true;
            if (self.nmt_state_machine.startup_flags & (1 << 1)) != 0 {
                info!("[MN] Sending NMTStartNode (Broadcast).");
                return self.serialize_and_prepare_action(payload::build_nmt_command_frame(
                    self,
                    NmtCommand::StartNode,
                    NodeId(C_ADR_BROADCAST_NODE_ID),
                ));
            } else if let Some(&node_id) = self.mandatory_nodes.first() {
                info!("[MN] Queuing NMTStartNode (Unicast).");
                self.pending_nmt_commands
                    .push((NmtCommand::StartNode, node_id));
            }
        } else if self.nmt_state() < NmtState::NmtOperational {
            self.initial_operational_actions_done = false;
        }

        // --- 2. Handle immediate, non-time-based follow-up actions ---
        match self.current_phase {
            CyclePhase::AwaitingMnAsyncSend => {
                if let Some((command, target_node)) = self.pending_nmt_commands.pop() {
                    info!(
                        "[MN] Sending queued NMT command {:?} for Node {}.",
                        command, target_node.0
                    );
                    self.current_phase = CyclePhase::Idle;
                    return self.serialize_and_prepare_action(payload::build_nmt_command_frame(
                        self,
                        command,
                        target_node,
                    ));
                } else if let Some(frame) = self.mn_async_send_queue.pop() {
                    info!("[MN] Sending queued generic async frame.");
                    self.current_phase = CyclePhase::Idle;
                    return self.serialize_and_prepare_action(frame);
                } else {
                    warn!("[MN] Was in AwaitingMnAsyncSend, but all send queues are empty.");
                    self.current_phase = CyclePhase::Idle;
                }
            }
            CyclePhase::SoCSent => {
                return self.advance_cycle_phase(current_time_us);
            }
            _ => {}
        }

        // --- 3. Handle time-based actions ---
        if self.nmt_state() == NmtState::NmtNotActive && self.next_tick_us.is_none() {
            let timeout_us = self.nmt_state_machine.wait_not_active_timeout as u64;
            self.next_tick_us = Some(current_time_us + timeout_us);
            return NodeAction::NoAction;
        }

        let deadline = self.next_tick_us.unwrap_or(u64::MAX);
        if current_time_us < deadline {
            return NodeAction::NoAction;
        }

        let mut action = NodeAction::NoAction;
        let mut schedule_next_cycle = true;

        if let Some(timeout_event) = self.pending_timeout_event.take() {
            let missed_node = self.current_polled_cn.unwrap_or(NodeId(0));
            warn!(
                "[MN] Timeout event {:?} for Node {}",
                timeout_event, missed_node.0
            );
            let dummy_frame = PowerlinkFrame::Soc(SocFrame::new(
                self.mac_address,
                Default::default(),
                NetTime {
                    seconds: 0,
                    nanoseconds: 0,
                },
                RelativeTime {
                    seconds: 0,
                    nanoseconds: 0,
                },
            ));
            events::handle_dll_event(self, timeout_event, &dummy_frame);

            if timeout_event == DllMsEvent::PresTimeout {
                if let Some(state) = self.node_states.get_mut(&missed_node) {
                    if *state >= CnState::Identified && *state != CnState::Stopped {
                        *state = CnState::Missing;
                    }
                }
                action = self.advance_cycle_phase(current_time_us);
                schedule_next_cycle = false;
            } else if timeout_event == DllMsEvent::AsndTimeout {
                self.current_phase = CyclePhase::Idle;
            }
        } else if self.nmt_state() == NmtState::NmtNotActive {
            info!("[MN] NotActive timeout expired. Proceeding to boot.");
            self.nmt_state_machine
                .process_event(NmtEvent::Timeout, &mut self.od);
        }

        let nmt_state = self.nmt_state();
        if action == NodeAction::NoAction && self.current_phase == CyclePhase::Idle {
            self.current_cycle_start_time_us = current_time_us;
            debug!(
                "[MN] Tick: Cycle start at {}us (State: {:?})",
                current_time_us, nmt_state
            );

            if nmt_state >= NmtState::NmtPreOperational1 {
                self.dll_error_manager.on_cycle_complete();
                if nmt_state >= NmtState::NmtPreOperational2 && self.multiplex_cycle_len > 0 {
                    self.current_multiplex_cycle =
                        (self.current_multiplex_cycle + 1) % self.multiplex_cycle_len;
                }
                action = match nmt_state {
                    NmtState::NmtPreOperational1 => self.advance_cycle_phase(current_time_us),
                    _ => {
                        self.current_phase = CyclePhase::SoCSent;
                        self.next_isoch_node_idx = 0;
                        self.serialize_and_prepare_action(payload::build_soc_frame(
                            self,
                            self.current_multiplex_cycle,
                            self.multiplex_cycle_len,
                        ))
                    }
                };
                schedule_next_cycle = false;
            }
        }

        if schedule_next_cycle {
            self.cycle_time_us = self.od.read_u32(OD_IDX_CYCLE_TIME, 0).unwrap_or(0) as u64;
            if self.cycle_time_us > 0 && self.nmt_state() >= NmtState::NmtPreOperational1 {
                let base_time = if self.current_cycle_start_time_us > 0
                    && current_time_us < self.current_cycle_start_time_us + self.cycle_time_us
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
                    debug!("[MN] Scheduling next cycle start at {}us", next_cycle_start);
                }
            }
        }
        action
    }

    /// Returns the NMT state of the node.
    fn nmt_state(&self) -> NmtState {
        self.nmt_state_machine.current_state()
    }

    /// Returns the absolute time of the next scheduled event.
    fn next_action_time(&self) -> Option<u64> {
        if matches!(
            self.current_phase,
            CyclePhase::SoCSent | CyclePhase::AwaitingMnAsyncSend
        ) {
            return Some(self.current_cycle_start_time_us);
        }
        if self.nmt_state() == NmtState::NmtNotActive && self.next_tick_us.is_none() {
            return Some(0);
        }
        self.next_tick_us
    }
}
