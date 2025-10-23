// In crates/powerlink-rs/src/node/mn.rs

use super::{Node, NodeAction};
use crate::common::{NetTime, RelativeTime};
use crate::frame::basic::MacAddress;
use crate::frame::{
    deserialize_frame,
    error::{DllError, DllErrorManager, LoggingErrorHandler, MnErrorCounters},
    ASndFrame, DllMsStateMachine, PowerlinkFrame, PResFrame,
    RequestedServiceId, ServiceId, SoAFrame, SocFrame,
};
use crate::nmt::mn_state_machine::MnNmtStateMachine;
use crate::nmt::state_machine::NmtStateMachine;
use crate::nmt::states::NmtState;
use crate::nmt::events::NmtEvent;
use crate::od::{Object, ObjectDictionary, ObjectValue};
use crate::pdo::{PdoMappingEntry};
use crate::types::{NodeId, EPLVersion};
use crate::{Codec, PowerlinkError};
use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;
use log::{debug, error, info, trace, warn};

// Constants for OD access
const OD_IDX_RPDO_COMM_PARAM_BASE: u16 = 0x1400;
const OD_IDX_RPDO_MAPP_PARAM_BASE: u16 = 0x1600;
const OD_IDX_NODE_ASSIGNMENT: u16 = 0x1F81;
const OD_IDX_CYCLE_TIME: u16 = 0x1006;
const OD_SUBIDX_PDO_COMM_NODEID: u8 = 1;
const OD_SUBIDX_PDO_COMM_VERSION: u8 = 2;

/// Internal state tracking for each configured CN.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CnState {
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
/// This struct owns and manages all protocol layers and state machines
/// required to manage a POWERLINK network.
pub struct ManagingNode<'s> {
    pub(super) od: ObjectDictionary<'s>,
    pub(super) nmt_state_machine: MnNmtStateMachine,
    dll_state_machine: DllMsStateMachine,
    dll_error_manager: DllErrorManager<MnErrorCounters, LoggingErrorHandler>,
    mac_address: MacAddress,
    last_soc_time_us: u64,
    /// Configured cycle time in microseconds.
    cycle_time_us: u64,
    /// Tracks the state of all configured CNs.
    node_states: BTreeMap<NodeId, CnState>,
    /// Tracks which mandatory CNs are expected.
    mandatory_nodes: Vec<NodeId>,
    /// Used to iterate through nodes during identification.
    last_ident_poll_node_id: NodeId,
    // TODO: Add async scheduler state
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
        let cycle_time_us = od
            .read_u32(OD_IDX_CYCLE_TIME, 0)
            .unwrap_or(0) as u64;
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
        info!("MN configured to manage {} nodes ({} mandatory).", node_states.len(), mandatory_nodes.len());

        let mut node = Self {
            od,
            nmt_state_machine,
            dll_state_machine: DllMsStateMachine::new(),
            dll_error_manager: DllErrorManager::new(MnErrorCounters::new(), LoggingErrorHandler),
            mac_address,
            last_soc_time_us: 0,
            cycle_time_us,
            node_states,
            mandatory_nodes,
            last_ident_poll_node_id: NodeId(0), // Start polling from the beginning
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
            self.nmt_state_machine.process_event(event, &mut self.od);
        }

        // 2. Update DLL state machine.
        // TODO: This logic needs to be fully implemented.
        // let dll_event = frame.dll_mn_event();
        // if let Some(errors) = self.dll_state_machine.process_event(dll_event, ...) {
        //    ...
        // }

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
                        self.check_bootup_state();
                    }
                } else {
                    warn!("[MN] Received IdentResponse from unconfigured Node {}.", node_id.0);
                }
            }
            ServiceId::StatusResponse => {
                // TODO: Update NMT state of the CN in our tracker
            }
            ServiceId::NmtRequest => {
                // TODO: Handle NMT request from CN and queue it in the async scheduler
                warn!("[MN] NMTRequest from CN {} not yet supported.", frame.source.0);
            }
            ServiceId::Sdo => {
                // TODO: Handle SDO (which for MN is usually a client response)
            }
            _ => {
                trace!("[MN] Ignoring ASnd with unhandled ServiceID {:?}", frame.service_id);
            }
        }
    }

    /// Checks if all mandatory nodes are identified to transition NMT state.
    fn check_bootup_state(&mut self) {
        if self.nmt_state() != NmtState::NmtPreOperational1 {
            return; // Only check this in PreOp1
        }

        let all_mandatory_identified = self.mandatory_nodes.iter().all(|node_id| {
            self.node_states.get(node_id) == Some(&CnState::Identified)
        });

        if all_mandatory_identified {
            info!("[MN] All mandatory nodes identified. Transitioning to PreOp2.");
            // This addresses TODO #5
            self.nmt_state_machine.process_event(
                NmtEvent::AllCnsIdentified,
                &mut self.od,
            );
        }
    }

    /// Reads RPDO mappings for a given source CN and writes
    /// data from the PRes payload into the MN's Object Dictionary.
    fn consume_pres_payload(&mut self, pres: &PResFrame) {
        if !pres.flags.rd {
            trace!(
                "Ignoring PRes payload from Node {}: RD flag is not set.",
                pres.source.0
            );
            return; // Data is not valid
        }

        if self.nmt_state() != NmtState::NmtOperational {
            trace!(
                "Ignoring PRes payload from Node {}: NMT state is not Operational.",
                pres.source.0
            );
            return; // Per spec, only consume in Operational
        }

        // Find the correct mapping for this source node
        let mut mapping_index = None;
        for i in 0..256 {
            let comm_param_index = OD_IDX_RPDO_COMM_PARAM_BASE + i as u16;
            if let Some(node_id_val) =
                self.od
                    .read_u8(comm_param_index, OD_SUBIDX_PDO_COMM_NODEID)
            {
                if node_id_val == pres.source.0 {
                    // Found the correct communication parameter object
                    let expected_version = self
                        .od
                        .read_u8(comm_param_index, OD_SUBIDX_PDO_COMM_VERSION)
                        .unwrap_or(0);

                    if expected_version != 0 && pres.pdo_version.0 != expected_version {
                        warn!(
                            "PRes PDO version mismatch for source Node {}. Expected {}, got {}. Ignoring payload.",
                            pres.source.0, expected_version, pres.pdo_version.0
                        );
                        let _ = self
                            .dll_error_manager
                            .handle_error(DllError::PdoMapVersion {
                                node_id: pres.source,
                            });
                        return;
                    }
                    mapping_index = Some(OD_IDX_RPDO_MAPP_PARAM_BASE + i as u16);
                    break;
                }
            }
        }

        let mapping_index = match mapping_index {
            Some(index) => index,
            None => {
                trace!("No RPDO mapping found for source Node {}.", pres.source.0);
                return;
            }
        };

        // We have a valid mapping, now process it
        if let Some(mapping_cow) = self.od.read(mapping_index, 0) {
            if let ObjectValue::Unsigned8(num_entries) = *mapping_cow {
                for i in 1..=num_entries {
                    if let Some(entry_cow) = self.od.read(mapping_index, i) {
                        if let ObjectValue::Unsigned64(raw_mapping) = *entry_cow {
                            let entry = PdoMappingEntry::from_u64(raw_mapping);
                            self.apply_rpdo_mapping_entry(&entry, &pres.payload, pres.source);
                        }
                    }
                }
            }
        }
    }

    /// Helper for `consume_pres_payload` to apply a single mapping entry.
    fn apply_rpdo_mapping_entry(
        &mut self,
        entry: &PdoMappingEntry,
        payload: &[u8],
        source_node_id: NodeId, // For error reporting
    ) {
        let (Some(offset), Some(length)) = (entry.byte_offset(), entry.byte_length()) else {
            warn!("Bit-level PDO mapping is not supported. Index: {}, SubIndex: {}.", entry.index, entry.sub_index);
            return;
        };

        if payload.len() < offset + length {
            warn!(
                "RPDO mapping for 0x{:04X}/{} from Node {} is out of bounds. Payload size: {}, expected at least {}.",
                entry.index, entry.sub_index, source_node_id.0, payload.len(), offset + length
            );
            let _ = self
                .dll_error_manager
                .handle_error(DllError::PdoPayloadShort {
                    node_id: source_node_id,
                });
            return;
        }

        let data_slice = &payload[offset..offset + length];
        let Some(type_template) = self.od.read(entry.index, entry.sub_index) else {
            warn!("RPDO mapping for 0x{:04X}/{} failed: OD entry not found.", entry.index, entry.sub_index);
            return;
        };

        match ObjectValue::deserialize(data_slice, &type_template) {
            Ok(value) => {
                trace!(
                    "Applying RPDO: Writing {:?} to 0x{:04X}/{}",
                    value,
                    entry.index,
                    entry.sub_index
                );
                if let Err(e) =
                    self.od
                        .write_internal(entry.index, entry.sub_index, value, false)
                {
                    warn!(
                        "Failed to write RPDO data to 0x{:04X}/{}: {:?}",
                        entry.index, entry.sub_index, e
                    );
                }
            }
            Err(e) => {
                warn!(
                    "Failed to deserialize RPDO data for 0x{:04X}/{}: {:?}",
                    entry.index, entry.sub_index, e
                );
            }
        }
    }

    /// Finds the next configured CN that has not been identified yet.
    fn find_next_node_to_identify(&mut self) -> Option<NodeId> {
        // Start iterating from the node *after* the last one polled
        let start_node_id = self.last_ident_poll_node_id.0.wrapping_add(1);
        
        let mut wrapped_around = false;
        let mut current_node_id = start_node_id;

        loop {
            // Handle wrap-around
            if current_node_id == 0 || current_node_id > 239 {
                current_node_id = 1;
            }
            if current_node_id == start_node_id {
                if wrapped_around {
                    break; // Full circle, no nodes found
                }
                wrapped_around = true;
            }
            
            let node_id = NodeId(current_node_id);
            if self.node_states.get(&node_id) == Some(&CnState::Unknown) {
                // Found a node to poll
                self.last_ident_poll_node_id = node_id;
                return Some(node_id);
            }

            current_node_id = current_node_id.wrapping_add(1);
        }

        None // No unidentified nodes left
    }

    /// Builds and serializes a SoC frame.
    fn build_soc_frame(&self) -> NodeAction {
        // TODO: Get real NetTime and RelativeTime
        let net_time = NetTime { seconds: 0, nanoseconds: 0 };
        let relative_time = RelativeTime { seconds: 0, nanoseconds: 0 };
        let soc_frame = SocFrame::new(
            self.mac_address,
            Default::default(),
            net_time,
            relative_time,
        );

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
        debug!("[MN] Building SoA(IdentRequest) for Node {}", target_node_id.0);
        let soa_frame = SoAFrame::new(
            self.mac_address,
            self.nmt_state(),
            Default::default(),
            RequestedServiceId::IdentRequest,
            target_node_id,
            EPLVersion(0x15), // TODO: Read from OD?
        );
        let mut buf = vec![0u8; 64]; // SoA is min frame size
        match soa_frame.serialize(&mut buf) {
            Ok(size) => {
                buf.truncate(size.max(60)); // Ensure min Ethernet frame size
                NodeAction::SendFrame(buf)
            }
            Err(e) => {
                error!("[MN] Failed to serialize SoA(IdentRequest) frame: {:?}", e);
                NodeAction::NoAction
            }
        }
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
            // We still process the frame to get the NMT event
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
        let nmt_state = self.nmt_state();

        // --- NotActive Timeout Check ---
        if nmt_state == NmtState::NmtNotActive {
            if self.last_soc_time_us == 0 {
                // First tick in this state, set timer
                self.last_soc_time_us = current_time_us;
                let timeout_ns = self.nmt_state_machine.wait_not_active_timeout;
                return NodeAction::SetTimer(timeout_ns as u64 / 1000); // ns to us
            }

            let elapsed_us = current_time_us.saturating_sub(self.last_soc_time_us);
            let timeout_us = self.nmt_state_machine.wait_not_active_timeout as u64 / 1000;

            if elapsed_us >= timeout_us {
                info!("[MN] NotActive timeout expired. No other MN detected. Proceeding to boot.");
                self.nmt_state_machine
                    .process_event(NmtEvent::Timeout, &mut self.od);
                self.last_soc_time_us = 0; // Reset for next cycle
                                           // Fall through to start the first action of the new state
            } else {
                return NodeAction::SetTimer(timeout_us - elapsed_us);
            }
        }

        // --- Cycle Timer Check (for cyclic states) ---
        // Re-read cycle_len_us as it might have changed if OD was updated
        self.cycle_time_us = self.od.read_u32(OD_IDX_CYCLE_TIME, 0).unwrap_or(0) as u64;
        
        if self.cycle_time_us == 0
            && matches!(
                nmt_state,
                NmtState::NmtOperational
                    | NmtState::NmtReadyToOperate
                    | NmtState::NmtPreOperational2
            )
        {
            warn!("MN cycle time (0x1006) is 0. Halting cycle.");
            return NodeAction::NoAction; // Cannot run cycle
        }

        let elapsed_since_soc = current_time_us.saturating_sub(self.last_soc_time_us);
        if elapsed_since_soc < self.cycle_time_us {
            // Not time for the next cycle yet
            return NodeAction::SetTimer(self.cycle_time_us - elapsed_since_soc);
        }

        // --- Time to start a new cycle ---
        self.last_soc_time_us = current_time_us;
        self.dll_error_manager.on_cycle_complete();

        debug!(
            "[MN] Tick: Cycle start (State: {:?})",
            self.nmt_state()
        );

        match self.nmt_state() {
            NmtState::NmtPreOperational1 => {
                // This is the MN boot-up state for discovering CNs.
                // We send SoA(IdentRequest) to the next unknown CN.
                // This directly addresses TODO #6.
                if let Some(node_to_poll) = self.find_next_node_to_identify() {
                    self.build_soa_ident_request(node_to_poll)
                } else {
                    // All nodes identified, but NMT state hasn't transitioned yet.
                    // Send a non-inviting SoA.
                    self.build_soa_ident_request(NodeId(0)) // NodeId 0 = NoService
                }
            }
            NmtState::NmtOperational
            | NmtState::NmtReadyToOperate
            | NmtState::NmtPreOperational2 => {
                // This is the start of the isochronous cycle.
                // It MUST begin with an SoC frame.
                // This addresses TODO #7 (partially).
                // The DLL state machine (TODO #2) will then take over to send PReqs.
                self.build_soc_frame()
            }
            _ => NodeAction::NoAction,
        }
    }

    fn nmt_state(&self) -> NmtState {
        self.nmt_state_machine.current_state()
    }
}

