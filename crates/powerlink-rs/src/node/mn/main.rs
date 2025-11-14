// crates/powerlink-rs/src/node/mn/main.rs
use alloc::collections::BTreeMap;

use super::events;
use super::state::{CyclePhase, MnContext};
use crate::PowerlinkError;
use crate::common::{NetTime, RelativeTime};
use crate::frame::basic::MacAddress;
use crate::frame::control::SocFrame;
use crate::frame::error::{DllErrorManager, LoggingErrorHandler, MnErrorCounters};
use crate::frame::ms_state_machine::DllMsStateMachine;
use crate::frame::{PowerlinkFrame, ServiceId, deserialize_frame}; // Added ServiceId
use crate::nmt::mn_state_machine::MnNmtStateMachine;
use crate::nmt::state_machine::NmtStateMachine;
use crate::nmt::states::NmtState;
use crate::node::{CoreNodeContext, Node, NodeAction, serialize_frame_action};
use crate::od::{Object, ObjectDictionary, ObjectValue, constants}; // Added Object, ObjectValue
use crate::sdo::client_manager::SdoClientManager;
use crate::sdo::command::SdoCommand;
use crate::sdo::sequence::SequenceLayerHeader;
use crate::sdo::server::SdoClientInfo;
#[cfg(feature = "sdo-udp")]
use crate::sdo::transport::UdpTransport;
use crate::sdo::transport::{AsndTransport, SdoTransport}; // Import trait
use crate::sdo::{EmbeddedSdoClient, EmbeddedSdoServer, SdoServer}; // Added embedded
use crate::types::{C_ADR_BROADCAST_NODE_ID, C_ADR_MN_DEF_NODE_ID, MessageType, NodeId};
use alloc::collections::BinaryHeap;
use alloc::vec::Vec;
use log::{error, info, trace, warn};

// --- Add imports for UDP SDO ---
#[cfg(feature = "sdo-udp")]
use crate::sdo::udp::deserialize_sdo_udp_payload;
#[cfg(feature = "sdo-udp")]
use crate::types::IpAddress;
// --- End of imports ---

// Import cycle functions
use super::cycle; // Import the cycle module
// --- NEW IMPORTS ---
use crate::nmt::events::{MnNmtCommandRequest, NmtManagingCommand, NmtStateCommand};
use crate::node::mn::state::NmtCommandData;
// --- END NEW IMPORTS ---

/// Represents a complete POWERLINK Managing Node (MN).
/// This struct is now a thin wrapper around the MnContext.
pub struct ManagingNode<'s> {
    pub context: MnContext<'s>,
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

        // Read cycle time (0x1006)
        let cycle_time_us = od.read_u32(constants::IDX_NMT_CYCLE_LEN_U32, 0).ok_or(
            PowerlinkError::ValidationError(
                // Use existing error type
                "Failed to read 0x1006 NMT_CycleLen_U32",
            ),
        )? as u64;

        // --- Initialize CN Management Info ---
        let (node_info, mandatory_nodes, isochronous_nodes, async_only_nodes, multiplex_assign) =
            cycle::parse_mn_node_lists(&od)?;

        // --- Initialize NMT Info Publish Configuration (0x1F9E) ---
        let mut publish_config = BTreeMap::new();
        if let Some(Object::Array(entries)) =
            od.read_object(constants::IDX_NMT_PUBLISH_CONFIG_AU32)
        {
            // Sub-index 0 is NumberOfEntries. Real entries start at 1.
            for (i, entry) in entries.iter().enumerate() {
                let sub_index = i as u8 + 1;
                if let ObjectValue::Unsigned32(config_val) = entry {
                    // Spec 7.2.1.1.18: Bits 0-7 = ServiceId, Bits 8-15 = MultiplexedCycle
                    let service_id_byte = (config_val & 0xFF) as u8;
                    let cycle_num = ((config_val >> 8) & 0xFF) as u8;

                    if cycle_num > 0 && service_id_byte > 0 {
                        match ServiceId::try_from(service_id_byte) {
                            Ok(service_id) => {
                                info!(
                                    "Configuring NMT Info Service: {:?} for Mux Cycle {}",
                                    service_id, cycle_num
                                );
                                publish_config.insert(cycle_num, service_id);
                            }
                            Err(_) => {
                                warn!(
                                    "Ignoring invalid ServiceId {:#04x} in 0x1F9E/{}",
                                    service_id_byte, sub_index
                                );
                            }
                        }
                    }
                }
            }
        } else {
            info!("NMT Publish Config (0x1F9E) not found. NMT Info Services disabled.");
        }

        // --- Initialize Core Context ---
        let core = CoreNodeContext {
            od, // OD is moved into context
            sdo_server: SdoServer::new(),
            // The CN's SdoClient is not used by the MN.
            sdo_client: Default::default(),
            mac_address,
            embedded_sdo_server: EmbeddedSdoServer::new(),
            embedded_sdo_client: EmbeddedSdoClient::new(),
        };

        // --- Initialize MnContext ---
        let context = MnContext {
            core,
            nmt_state_machine,
            dll_state_machine: DllMsStateMachine::new(), // Removed node_id
            dll_error_manager: DllErrorManager::new(MnErrorCounters::new(), LoggingErrorHandler),
            asnd_transport: AsndTransport,
            #[cfg(feature = "sdo-udp")]
            udp_transport: UdpTransport,
            cycle_time_us,
            multiplex_cycle_len: 8, // Default, TODO: Read from 0x1F98
            multiplex_assign,
            publish_config, // Add the new config map
            current_multiplex_cycle: 0,
            node_info,
            mandatory_nodes,
            isochronous_nodes,
            async_only_nodes,
            arp_cache: BTreeMap::new(),
            next_isoch_node_idx: 0,
            current_phase: CyclePhase::Idle,
            current_polled_cn: None,
            async_request_queue: BinaryHeap::new(),
            pending_er_requests: Vec::new(),
            pending_status_requests: Vec::new(),
            pending_nmt_commands: Vec::new(),
            mn_async_send_queue: Vec::new(),
            sdo_client_manager: SdoClientManager::new(), // Changed
            last_ident_poll_node_id: NodeId(0),
            last_status_poll_node_id: NodeId(0),
            next_tick_us: None,
            pending_timeout_event: None,
            current_cycle_start_time_us: 0,
            initial_operational_actions_done: false,
        };

        Ok(Self { context })
    }

    /// Private helper to process a fully deserialized POWERLINK frame.
    /// This was missing.
    fn process_powerlink_frame(
        &mut self,
        frame: PowerlinkFrame,
        current_time_us: u64,
    ) -> NodeAction {
        match frame {
            PowerlinkFrame::PRes(pres_frame) => {
                events::process_frame(
                    &mut self.context,
                    PowerlinkFrame::PRes(pres_frame),
                    current_time_us,
                );
            }
            PowerlinkFrame::ASnd(asnd_frame) => {
                // process_asnd_frame expects the enum
                return self.process_asnd_frame(PowerlinkFrame::ASnd(asnd_frame), current_time_us);
            }
            _ => {
                // SoC, PReq, SoA are sent *by* the MN, not received by it.
                // Receiving one is a network error (e.g., another MN).
                let frame_type_for_log = match &frame {
                    PowerlinkFrame::Soc(_) => MessageType::SoC,
                    PowerlinkFrame::PReq(_) => MessageType::PReq,
                    PowerlinkFrame::PRes(_) => MessageType::PRes,
                    PowerlinkFrame::SoA(_) => MessageType::SoA,
                    PowerlinkFrame::ASnd(_) => MessageType::ASnd,
                };
                warn!(
                    "MN received unexpected frame type: {:?}",
                    frame_type_for_log
                );
                // TODO: Handle multiple MNs on network error
            }
        }
        NodeAction::NoAction
    }

    /// Private helper to deserialize and dispatch an Ethernet frame's payload.
    /// This was the missing method `run_cycle` was trying to call.
    fn process_ethernet_frame(&mut self, frame_bytes: &[u8], current_time_us: u64) -> NodeAction {
        // --- 1. Update NMT State (for Resetting) ---
        // NmtResetting is not a valid state, use NmtResetCommunication
        if self.context.nmt_state_machine.current_state() == NmtState::NmtGsResetCommunication {
            self.context.nmt_state_machine.process_event(
                crate::nmt::events::NmtEvent::Error, // Use a generic event
                &mut self.context.core.od,
            );
            return NodeAction::NoAction;
        }

        // --- 2. Deserialize Frame ---
        let frame = match deserialize_frame(frame_bytes) {
            Ok(frame) => frame,
            Err(PowerlinkError::InvalidEthernetFrame) => {
                // Ignore non-POWERLINK frames
                trace!("Ignoring non-POWERLINK frame");
                return NodeAction::NoAction;
            }
            Err(e) => {
                warn!("Failed to deserialize frame: {:?}", e);
                return NodeAction::NoAction;
            }
        };

        // --- 3. Process Frame ---
        self.process_powerlink_frame(frame, current_time_us)
    }

    /// Helper function to process ASnd frames, which might contain
    /// SDO, IdentResponse, StatusResponse, or other services.
    fn process_asnd_frame(
        &mut self,
        asnd_frame: PowerlinkFrame, // Pass enum, not just inner frame
        current_time_us: u64,
    ) -> NodeAction {
        let (asnd_service_id, asnd_dest_node_id, asnd_source_node_id) = match &asnd_frame {
            PowerlinkFrame::ASnd(f) => (f.service_id, f.destination, f.source),
            _ => return NodeAction::NoAction, // Should not happen
        };

        // Check if this is an SDO frame
        if asnd_service_id == ServiceId::Sdo {
            // Check if it's an SDO response *for us* (the MN)
            if asnd_dest_node_id == self.context.nmt_state_machine.node_id() {
                // *** INCREMENT SDO RX COUNTER (ASnd Response) ***
                self.context.core.od.increment_counter(
                    constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
                    constants::SUBIDX_DIAG_NMT_COUNT_SDO_RX,
                );
                trace!(
                    "Received SDO ASnd response from Node {}.",
                    asnd_source_node_id.0
                );
                // --- SDO Client Manager Logic ---
                let payload = match &asnd_frame {
                    PowerlinkFrame::ASnd(f) => &f.payload,
                    _ => unreachable!(),
                };
                if payload.len() < 8 {
                    // Min SDO payload is SeqHdr(4) + CmdHdr(4)
                    warn!("Received SDO frame with invalid payload length. Ignoring.");
                    return NodeAction::NoAction;
                }
                // Deserialize Sequence Layer
                match SequenceLayerHeader::deserialize(&payload[0..4]) {
                    Ok(seq_header) => {
                        // Deserialize Command Layer
                        match SdoCommand::deserialize(&payload[4..]) {
                            Ok(cmd) => {
                                self.context.sdo_client_manager.handle_response(
                                    asnd_source_node_id,
                                    seq_header,
                                    cmd,
                                    // Removed unused parameters
                                );
                            }
                            Err(e) => {
                                error!(
                                    "Failed to deserialize SDO command from Node {}: {:?}",
                                    asnd_source_node_id.0, e
                                );
                            }
                        }
                    }
                    Err(e) => {
                        error!(
                            "Failed to deserialize SDO sequence header from Node {}: {:?}",
                            asnd_source_node_id.0, e
                        );
                    }
                }
                // SDO Client Manager doesn't send immediate responses
                return NodeAction::NoAction;
            // Check if it's an SDO request *to* us (the MN's server)
            } else if asnd_dest_node_id == NodeId(C_ADR_MN_DEF_NODE_ID) {
                // *** INCREMENT SDO RX COUNTER (ASnd Request) ***
                self.context.core.od.increment_counter(
                    constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
                    constants::SUBIDX_DIAG_NMT_COUNT_SDO_RX,
                );
                trace!(
                    "Received SDO/ASnd request from Node {}.",
                    asnd_source_node_id.0
                );
                let (source_mac, payload) = match asnd_frame {
                    PowerlinkFrame::ASnd(f) => (f.eth_header.source_mac, f.payload),
                    _ => unreachable!(),
                };

                let client_info = SdoClientInfo::Asnd {
                    source_node_id: asnd_source_node_id,
                    source_mac,
                };
                return self.handle_sdo_server_request(&payload, client_info, current_time_us);
            }
        }

        // --- Pass to MN Event Handler ---
        // This is for IdentResponse, StatusResponse, etc.
        events::process_frame(&mut self.context, asnd_frame, current_time_us);
        NodeAction::NoAction
    }

    /// Handles an SDO request for the MN's SDO server.
    fn handle_sdo_server_request(
        &mut self,
        sdo_payload: &[u8],
        client_info: SdoClientInfo,
        current_time_us: u64,
    ) -> NodeAction {
        match self.context.core.sdo_server.handle_request(
            sdo_payload,
            client_info,
            &mut self.context.core.od,
            current_time_us,
        ) {
            Ok(response_data) => {
                match self
                    .context
                    .asnd_transport
                    .build_response(response_data, &self.context)
                {
                    Ok(action) => {
                        // *** INCREMENT SDO TX COUNTER (ASnd Response) ***
                        self.context.core.od.increment_counter(
                            constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
                            constants::SUBIDX_DIAG_NMT_COUNT_SDO_TX,
                        );
                        action
                    }
                    Err(e) => {
                        error!("Failed to build SDO/ASnd response: {:?}", e);
                        NodeAction::NoAction
                    }
                }
            }
            Err(e) => {
                error!("SDO server error (ASnd): {:?}", e);
                NodeAction::NoAction
            }
        }
    }

    /// Handles periodic timer events for the node.
    fn handle_tick(&mut self, current_time_us: u64) -> NodeAction {
        // --- 0. Check for Cycle Start ---
        let time_since_last_cycle =
            current_time_us.saturating_sub(self.context.current_cycle_start_time_us);
        let current_nmt_state = self.context.nmt_state_machine.current_state();

        if time_since_last_cycle >= self.context.cycle_time_us
            && current_nmt_state >= NmtState::NmtPreOperational2
            && self.context.current_phase == CyclePhase::Idle
        {
            trace!("[MN] Cycle time elapsed. Starting new cycle.");
            return cycle::start_cycle(&mut self.context, current_time_us);
        }

        // --- 1. Check for SDO Client Timeouts ---
        if let Some((target_node_id, seq, cmd)) = self
            .context
            .sdo_client_manager
            .tick(current_time_us, &self.context.core.od)
        {
            warn!(
                "SDO Client tick generated frame (timeout/abort) for Node {}.",
                target_node_id.0
            );
            // An SDO client timeout/abort needs to send a frame.
            // This function needs to exist in scheduler or payload
            match cycle::build_sdo_asnd_request(&self.context, target_node_id, seq, cmd) {
                // Corrected: cycle::
                Ok(frame) => {
                    // *** INCREMENT SDO TX COUNTER (ASnd Client Abort) ***
                    self.context.core.od.increment_counter(
                        constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
                        constants::SUBIDX_DIAG_NMT_COUNT_SDO_TX,
                    );
                    return serialize_frame_action(frame, &mut self.context)
                        .unwrap_or(NodeAction::NoAction);
                }
                Err(e) => error!("Failed to build SDO client tick frame: {:?}", e),
            }
        }

        // --- 2. Check for SDO Server Timeouts ---
        if let Some(deadline) = self.context.core.sdo_server.next_action_time() {
            if current_time_us >= deadline {
                match self
                    .context
                    .core
                    .sdo_server
                    .tick(current_time_us, &self.context.core.od)
                {
                    Ok(Some(response_data)) => {
                        // SDO server timed out, needs to send an Abort.
                        // This assumes ASnd transport for timeouts.
                        warn!("SDO Server tick generated abort frame.");
                        let build_result = match response_data.client_info {
                            SdoClientInfo::Asnd { .. } => {
                                // *** INCREMENT SDO TX COUNTER (ASnd Server Abort) ***
                                self.context.core.od.increment_counter(
                                    constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
                                    constants::SUBIDX_DIAG_NMT_COUNT_SDO_TX,
                                );
                                self.context
                                    .asnd_transport
                                    .build_response(response_data, &self.context)
                            }
                            #[cfg(feature = "sdo-udp")]
                            SdoClientInfo::Udp { .. } => {
                                // *** INCREMENT SDO TX COUNTER (UDP Server Abort) ***
                                self.context.core.od.increment_counter(
                                    constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
                                    constants::SUBIDX_DIAG_NMT_COUNT_SDO_TX,
                                );
                                self.context
                                    .udp_transport
                                    .build_response(response_data, &self.context)
                            }
                        };
                        match build_result {
                            Ok(action) => return action,
                            Err(e) => {
                                error!("Failed to build SDO/ASnd abort response: {:?}", e);
                            }
                        }
                    }
                    Ok(None) => {} // Tick processed, no action
                    Err(e) => error!("SDO server tick error: {:?}", e),
                }
            }
        }

        // --- 3. Check for NMT/Scheduler Timeouts ---
        let deadline_passed = self
            .context
            .next_tick_us
            .is_some_and(|deadline| current_time_us >= deadline);

        if !deadline_passed {
            return NodeAction::NoAction; // No timer events
        }

        // A deadline has passed
        trace!(
            "Tick deadline reached at {}us (Deadline was {:?})",
            current_time_us, self.context.next_tick_us
        );
        self.context.next_tick_us = None; // Consume deadline

        // --- Handle PRes Timeout ---
        if let Some(event) = self.context.pending_timeout_event.take() {
            // This is a PRes timeout
            warn!(
                "[MN] PRes timeout for Node {:?}.",
                self.context.current_polled_cn
            );
            events::handle_dll_event(
                &mut self.context,
                event,
                // Corrected: Use SocFrame::new with default values
                &PowerlinkFrame::Soc(SocFrame::new(
                    Default::default(),
                    Default::default(),
                    NetTime {
                        seconds: 0,
                        nanoseconds: 0,
                    }, // Use NetTime::default() if available
                    RelativeTime {
                        seconds: 0,
                        nanoseconds: 0,
                    }, // Use RelativeTime::default() if available
                )),
            );
            // A PRes timeout means we must advance the cycle.
            return cycle::advance_cycle_phase(&mut self.context, current_time_us);
        } else {
            // This is a general NMT tick (e.g., for async SDO polls)
            cycle::tick(&mut self.context, current_time_us) // Corrected: cycle::
        }
    }

    // --- New Public API Methods ---

    /// Queues an NMT state command to be sent to a target CN or broadcast.
    ///
    /// # Arguments
    /// * `command` - The NMT state command to send (e.g., `NmtStateCommand::StartNode`).
    /// * `target` - The `NodeId` of the target CN, or `NodeId(C_ADR_BROADCAST_NODE_ID)` for broadcast.
    pub fn queue_nmt_state_command(&mut self, command: NmtStateCommand, target: NodeId) {
        info!(
            "Queueing NMT State Command: {:?} for Node {}",
            command, target.0
        );
        self.context.pending_nmt_commands.push((
            MnNmtCommandRequest::State(command),
            target,
            NmtCommandData::None,
        ));
    }

    /// Queues an NMTNetHostNameSet command to be sent to a target CN.
    /// (Reference: EPSG DS 301, Section 7.3.2.1.1)
    ///
    /// # Arguments
    /// * `target` - The `NodeId` of the target CN. Must be a unicast address.
    /// * `hostname` - The hostname to set (max 32 bytes).
    pub fn set_hostname(
        &mut self,
        target: NodeId,
        hostname: alloc::string::String,
    ) -> Result<(), PowerlinkError> {
        info!(
            "Queueing NMTNetHostNameSet for Node {}: {}",
            target.0, hostname
        );
        if hostname.len() > 32 {
            return Err(PowerlinkError::ValidationError(
                "Hostname exceeds 32 characters",
            ));
        }
        if target.0 == C_ADR_BROADCAST_NODE_ID {
            return Err(PowerlinkError::ValidationError(
                "NmtNetHostNameSet must be unicast",
            ));
        }
        self.context.pending_nmt_commands.push((
            MnNmtCommandRequest::Managing(NmtManagingCommand::NmtNetHostNameSet),
            target,
            NmtCommandData::HostName(hostname),
        ));
        Ok(())
    }

    /// Queues an NMTFlushArpEntry command to be sent as a broadcast.
    /// (Reference: EPSG DS 301, Section 7.3.2.1.2)
    ///
    /// # Arguments
    /// * `target` - The `NodeId` to flush, or `NodeId(C_ADR_BROADCAST_NODE_ID)` to flush all.
    pub fn flush_arp_entry(&mut self, target: NodeId) {
        info!("Queueing NMTFlushArpEntry for Node {}", target.0);
        self.context.pending_nmt_commands.push((
            MnNmtCommandRequest::Managing(NmtManagingCommand::NmtFlushArpEntry),
            NodeId(crate::types::C_ADR_BROADCAST_NODE_ID), // Command is always broadcast
            NmtCommandData::FlushArp(target), // Payload contains the target to flush
        ));
    }

    /// Initiates an SDO Read (Upload) transfer from a target CN.
    pub fn read_object(
        &mut self,
        target: NodeId,
        index: u16,
        sub_index: u8,
        current_time_us: u64,
    ) -> Result<(), PowerlinkError> {
        info!(
            "Queueing SDO Read from Node {} for 0x{:04X}/{}",
            target.0, index, sub_index
        );
        self.context.sdo_client_manager.read_object_by_index(
            target,
            index,
            sub_index,
            current_time_us,
            &self.context.core.od,
        )
    }

    /// Initiates an SDO Write (Download) transfer to a target CN.
    pub fn write_object(
        &mut self,
        target: NodeId,
        index: u16,
        sub_index: u8,
        data: Vec<u8>,
        current_time_us: u64,
    ) -> Result<(), PowerlinkError> {
        info!(
            "Queueing SDO Write to Node {} for 0x{:04X}/{} ({} bytes)",
            target.0,
            index,
            sub_index,
            data.len()
        );
        self.context.sdo_client_manager.write_object_by_index(
            target,
            index,
            sub_index,
            data,
            current_time_us,
            &self.context.core.od,
        )
    }

    // --- End of New Public API ---

    /// Processes a UDP datagram payload for SDO over UDP.
    #[cfg(feature = "sdo-udp")]
    fn process_udp_datagram(
        &mut self,
        payload: &[u8],                     // Corrected: buffer
        source_ip: crate::types::IpAddress, // Corrected: ip
        source_port: u16,                   // Corrected: port
        current_time_us: u64,
    ) -> NodeAction {
        trace!(
            "Processing UDP datagram from {}:{} ({} bytes)",
            core::net::Ipv4Addr::from(source_ip),
            source_port,
            payload.len()
        );

        // Check if this is an SDO/UDP frame
        match deserialize_sdo_udp_payload(payload) {
            Ok((seq_header, cmd)) => {
                // *** INCREMENT SDO RX COUNTER (UDP Request) ***
                self.context.core.od.increment_counter(
                    constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
                    constants::SUBIDX_DIAG_NMT_COUNT_SDO_RX,
                );

                // This is an SDO request *to* the MN's SDO server.
                let client_info = SdoClientInfo::Udp {
                    source_ip,
                    source_port,
                };
                // Re-serialize the SDO payload (SeqHdr + Cmd) for the SdoServer.
                let mut sdo_payload = vec![0u8; payload.len()]; // Max possible size
                let seq_len = seq_header.serialize(&mut sdo_payload).unwrap_or(0);
                let cmd_len = cmd.serialize(&mut sdo_payload[seq_len..]).unwrap_or(0);
                let total_sdo_len = seq_len + cmd_len;
                sdo_payload.truncate(total_sdo_len);

                // --- Call SDO Server and build UDP response ---
                match self.context.core.sdo_server.handle_request(
                    &sdo_payload,
                    client_info,
                    &mut self.context.core.od,
                    current_time_us,
                ) {
                    Ok(response_data) => {
                        match self
                            .context
                            .udp_transport
                            .build_response(response_data, &self.context)
                        {
                            Ok(action) => action, // This will be NodeAction::SendUdp
                            Err(e) => {
                                error!("Failed to build SDO/UDP response: {:?}", e);
                                NodeAction::NoAction
                            }
                        }
                    }
                    Err(e) => {
                        error!("SDO server error (UDP): {:?}", e);
                        NodeAction::NoAction
                    }
                }
            }
            Err(e) => {
                warn!("Failed to deserialize SDO/UDP payload: {:?}", e);
                NodeAction::NoAction
            }
        }
    }
}

// Corrected implementation of the Node trait
impl<'s> Node for ManagingNode<'s> {
    #[cfg(feature = "sdo-udp")]
    fn run_cycle(
        &mut self,
        ethernet_frame: Option<&[u8]>,
        udp_datagram: Option<(&[u8], IpAddress, u16)>,
        current_time_us: u64,
    ) -> NodeAction {
        // --- Priority 1: Ethernet Frames ---
        if let Some(buffer) = ethernet_frame {
            // Check for POWERLINK EtherType
            if buffer.len() >= 14
                && buffer[12..14] == crate::types::C_DLL_ETHERTYPE_EPL.to_be_bytes()
            {
                let action = self.process_ethernet_frame(buffer, current_time_us);
                if action != NodeAction::NoAction {
                    // SDO Tx counter (for ASnd) is handled inside process_ethernet_frame
                    return action;
                }
            }
            // Ignore non-POWERLINK Ethernet frames
        }

        // --- Priority 2: UDP Datagrams ---
        if let Some((buffer, ip, port)) = udp_datagram {
            let action = self.process_udp_datagram(buffer, ip, port, current_time_us);
            if let NodeAction::SendUdp { .. } = action {
                // *** INCREMENT SDO TX COUNTER (UDP Response) ***
                self.context.core.od.increment_counter(
                    constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
                    constants::SUBIDX_DIAG_NMT_COUNT_SDO_TX,
                );
            }
            if action != NodeAction::NoAction {
                return action;
            }
        }

        // --- Priority 3: Internal Ticks ---
        let tick_action = self.handle_tick(current_time_us);
        // SDO Tx counter (for tick-based aborts/ASnd responses) is handled inside handle_tick
        tick_action
    }

    #[cfg(not(feature = "sdo-udp"))]
    fn run_cycle(&mut self, ethernet_frame: Option<&[u8]>, current_time_us: u64) -> NodeAction {
        // --- Priority 1: Ethernet Frames ---
        if let Some(buffer) = ethernet_frame {
            // Check for POWERLINK EtherType
            if buffer.len() >= 14
                && buffer[12..14] == crate::types::C_DLL_ETHERTYPE_EPL.to_be_bytes()
            {
                let action = self.process_ethernet_frame(buffer, current_time_us);
                if action != NodeAction::NoAction {
                    // SDO Tx counter (for ASnd) is handled inside process_ethernet_frame
                    return action;
                }
            }
            // Ignore non-POWERLINK Ethernet frames
        }

        // --- Priority 2: Internal Ticks ---
        let tick_action = self.handle_tick(current_time_us);
        // SDO Tx counter (for tick-based aborts/ASnd responses) is handled inside handle_tick
        tick_action
    }

    /// Returns the NMT state of the node.
    fn nmt_state(&self) -> NmtState {
        self.context.nmt_state_machine.current_state()
    }

    /// Returns the absolute time of the next scheduled event.
    fn next_action_time(&self) -> Option<u64> {
        if matches!(
            self.context.current_phase,
            CyclePhase::SoCSent | CyclePhase::AwaitingMnAsyncSend
        ) {
            return Some(self.context.current_cycle_start_time_us);
        }
        if self.nmt_state() == NmtState::NmtNotActive && self.context.next_tick_us.is_none() {
            return Some(0); // Poll immediately if in NotActive
        }

        // Check all possible timers
        let sdo_server_time = self.context.core.sdo_server.next_action_time();
        let sdo_client_time = self
            .context
            .sdo_client_manager
            .next_action_time(&self.context.core.od);
        let nmt_time = self.context.next_tick_us;

        // --- NEW: Add Cycle Timer ---
        let mut cycle_start_time = None;
        if self.context.nmt_state_machine.current_state() >= NmtState::NmtPreOperational2
            && self.context.current_phase == CyclePhase::Idle
        {
            cycle_start_time =
                Some(self.context.current_cycle_start_time_us + self.context.cycle_time_us);
        }
        // --- End NEW ---

        [sdo_server_time, sdo_client_time, nmt_time, cycle_start_time] // Add new timer
            .iter()
            .filter_map(|&t| t)
            .min()
    }
}