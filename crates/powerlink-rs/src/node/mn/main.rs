// crates/powerlink-rs/src/node/mn/main.rs
use super::events;
use super::scheduler;
use super::state::{CnInfo, CyclePhase, MnContext};
use crate::PowerlinkError;
use crate::frame::basic::MacAddress;
use crate::frame::{
    DllMsStateMachine, PowerlinkFrame, ServiceId, deserialize_frame,
    error::{DllError, DllErrorManager, LoggingErrorHandler, MnErrorCounters},
};
use crate::nmt::mn_state_machine::MnNmtStateMachine;
use crate::nmt::state_machine::NmtStateMachine;
use crate::nmt::states::NmtState;
use crate::node::{CoreNodeContext, Node, NodeAction};
use crate::od::{Object, ObjectDictionary, ObjectValue};
use crate::sdo::SdoTransport;
use crate::sdo::server::SdoClientInfo;
use crate::sdo::transport::AsndTransport;
#[cfg(feature = "sdo-udp")]
use crate::sdo::transport::UdpTransport;
use crate::sdo::{SdoClient, SdoServer};
use crate::types::NodeId;
use alloc::collections::{BTreeMap, BinaryHeap};
use alloc::vec::Vec;
use log::{debug, error, info, warn};

// --- Add imports for UDP SDO ---
#[cfg(feature = "sdo-udp")]
use crate::sdo::udp::deserialize_sdo_udp_payload;
#[cfg(feature = "sdo-udp")]
use crate::types::IpAddress;
// --- End of imports ---

// Constants for OD access used in this file.
const OD_IDX_NODE_ASSIGNMENT: u16 = 0x1F81;
const OD_IDX_CYCLE_TIME: u16 = 0x1006;
const OD_IDX_CYCLE_TIMING_REC: u16 = 0x1F98;
const OD_SUBIDX_MULTIPLEX_CYCLE_LEN: u8 = 7;
const OD_IDX_MULTIPLEX_ASSIGN: u16 = 0x1F9B;

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
        let cycle_time_us = od.read_u32(OD_IDX_CYCLE_TIME, 0).unwrap_or(0) as u64;
        let multiplex_cycle_len = od
            .read_u8(OD_IDX_CYCLE_TIMING_REC, OD_SUBIDX_MULTIPLEX_CYCLE_LEN)
            .unwrap_or(0);

        let mut node_info = BTreeMap::new();
        let mut mandatory_nodes = Vec::new();
        let mut isochronous_nodes = Vec::new();
        let mut async_only_nodes = Vec::new();
        let mut multiplex_assign = BTreeMap::new();

        if let Some(Object::Array(entries)) = od.read_object(OD_IDX_NODE_ASSIGNMENT) {
            for (i, entry) in entries.iter().enumerate().skip(1) {
                if let ObjectValue::Unsigned32(assignment) = entry {
                    if (assignment & 1) != 0 {
                        if let Ok(node_id) = NodeId::try_from(i as u8) {
                            node_info.insert(node_id, CnInfo::default());
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

        // --- Instantiate CoreNodeContext ---
        let core_context = CoreNodeContext {
            od,
            mac_address,
            sdo_server: SdoServer::new(),
            sdo_client: SdoClient::new(),
        };

        info!(
            "MN configured to manage {} nodes ({} mandatory, {} isochronous, {} async-only). Multiplex Cycle Length: {}",
            node_info.len(),
            mandatory_nodes.len(),
            isochronous_nodes.len(),
            async_only_nodes.len(),
            multiplex_cycle_len
        );

        let mut context = MnContext {
            core: core_context, // Use the new core context
            nmt_state_machine,
            dll_state_machine: DllMsStateMachine::new(),
            dll_error_manager: DllErrorManager::new(MnErrorCounters::new(), LoggingErrorHandler),
            asnd_transport: AsndTransport,
            #[cfg(feature = "sdo-udp")]
            udp_transport: UdpTransport,
            cycle_time_us,
            multiplex_cycle_len,
            multiplex_assign,
            current_multiplex_cycle: 0,
            node_info,
            mandatory_nodes,
            isochronous_nodes,
            async_only_nodes,
            next_isoch_node_idx: 0,
            current_phase: CyclePhase::Idle,
            current_polled_cn: None,
            async_request_queue: BinaryHeap::new(),
            pending_er_requests: Vec::new(),
            pending_status_requests: Vec::new(),
            pending_nmt_commands: Vec::new(),
            mn_async_send_queue: Vec::new(),
            pending_sdo_client_requests: Vec::new(),
            last_ident_poll_node_id: NodeId(0),
            last_status_poll_node_id: NodeId(0),
            next_tick_us: None,
            pending_timeout_event: None,
            current_cycle_start_time_us: 0,
            initial_operational_actions_done: false,
        };

        context
            .nmt_state_machine
            .run_internal_initialisation(&mut context.core.od);

        Ok(Self { context })
    }

    /// Queues a generic asynchronous frame to be sent by the MN.
    pub fn queue_mn_async_frame(&mut self, frame: PowerlinkFrame) {
        info!("[MN] Queuing MN-initiated async frame: {:?}", frame);
        self.context.mn_async_send_queue.push(frame);
    }

    /// Queues an SDO request payload to be sent from the MN to a specific CN.
    /// The payload must be a complete SDO payload (Sequence Header + Command Layer).
    pub fn queue_sdo_request(&mut self, target_node_id: NodeId, sdo_payload: Vec<u8>) {
        info!(
            "[MN] Queuing SDO request for Node {} ({} bytes)",
            target_node_id.0,
            sdo_payload.len()
        );
        self.context
            .pending_sdo_client_requests
            .push((target_node_id, sdo_payload));
    }

    /// Queues a request to send an SoA(StatusRequest) with the ER (Exception Reset)
    /// flag set to `true` to a specific CN.
    /// This allows an application to manually clear a CN's error signaling state.
    pub fn queue_reset_cn_error_signaling(&mut self, node_id: NodeId) {
        info!("[MN] Queuing Exception Reset for Node {}", node_id.0);
        // Avoid adding duplicates
        if !self.context.pending_er_requests.contains(&node_id) {
            self.context.pending_er_requests.push(node_id);
        }
    }

    /// Internal function to process a deserialized `PowerlinkFrame`.
    fn process_ethernet_frame(
        &mut self,
        buffer: &[u8],
        current_time_us: u64,
    ) -> NodeAction {
        if self.nmt_state() == NmtState::NmtNotActive
            && buffer.get(12..14) == Some(&crate::types::C_DLL_ETHERTYPE_EPL.to_be_bytes())
            && buffer.get(6..12) != Some(&self.context.core.mac_address.0)
        {
            warn!("[MN] POWERLINK frame detected while in NotActive state from another MN.");
            self.context
                .dll_error_manager
                .handle_error(DllError::MultipleMn);
        }

        match deserialize_frame(buffer) {
            Ok(frame) => {
                // --- Handle SDO ASnd frames before generic processing ---
                if let PowerlinkFrame::ASnd(ref asnd_frame) = frame {
                    if asnd_frame.destination == self.context.nmt_state_machine.node_id
                        && asnd_frame.service_id == ServiceId::Sdo
                    {
                        debug!("[MN] Received SDO/ASnd for processing.");
                        let sdo_payload = &asnd_frame.payload;
                        let client_info = SdoClientInfo::Asnd {
                            source_node_id: asnd_frame.source,
                            source_mac: asnd_frame.eth_header.source_mac,
                        };
                        match self.context.core.sdo_server.handle_request(
                            sdo_payload,
                            client_info,
                            &mut self.context.core.od,
                            current_time_us,
                        ) {
                            Ok(response_data) => {
                                return match self
                                    .context
                                    .asnd_transport
                                    .build_response(response_data, &self.context)
                                {
                                    Ok(action) => action,
                                    Err(e) => {
                                        error!("Failed to build SDO/ASnd response: {:?}", e);
                                        NodeAction::NoAction
                                    }
                                };
                            }
                            Err(e) => {
                                error!("SDO server error (ASnd): {:?}", e);
                                return NodeAction::NoAction;
                            }
                        }
                    }
                }

                // If not an SDO frame for us, proceed with general event handling.
                events::process_frame(&mut self.context, frame, current_time_us);
                // General event processing does not return an immediate action.
                NodeAction::NoAction
            }
            Err(e) if e != PowerlinkError::InvalidEthernetFrame => {
                // Log any POWERLINK-specific deserialization errors.
                warn!("[MN] Error during frame deserialization: {:?}", e);
                self.context
                    .dll_error_manager
                    .handle_error(DllError::InvalidFormat);
                NodeAction::NoAction
            }
            _ => {
                // Ignore non-POWERLINK frames silently.
                NodeAction::NoAction
            }
        }
    }

    /// Processes a UDP datagram payload for SDO over UDP.
    #[cfg(feature = "sdo-udp")]
    fn process_udp_datagram(
        &mut self,
        buffer: &[u8],
        source_ip: IpAddress,
        source_port: u16,
        current_time_us: u64,
    ) -> NodeAction {
        debug!(
            "[MN] Received UDP datagram ({} bytes) from {}:{}",
            buffer.len(),
            core::net::Ipv4Addr::from(source_ip),
            source_port
        );

        // 1. Deserialize the SDO payload from the UDP datagram
        let (seq_header, cmd) = match deserialize_sdo_udp_payload(buffer) {
            Ok((seq, cmd)) => (seq, cmd),
            Err(e) => {
                warn!("[MN] Failed to deserialize SDO/UDP payload: {:?}", e);
                return NodeAction::NoAction;
            }
        };

        // 2. Define the client info for the SDO server
        let client_info = SdoClientInfo::Udp {
            source_ip,
            source_port,
        };

        // 3. Re-serialize the SDO payload (SeqHdr + Cmd) for the SdoServer.
        let mut sdo_payload = vec![0u8; buffer.len()]; // Max possible size
        let seq_len = seq_header.serialize(&mut sdo_payload).unwrap_or(0);
        let cmd_len = cmd.serialize(&mut sdo_payload[seq_len..]).unwrap_or(0);
        let total_sdo_len = seq_len + cmd_len;
        sdo_payload.truncate(total_sdo_len);

        // 4. Handle the SDO command
        match self.context.core.sdo_server.handle_request(
            &sdo_payload,
            client_info,
            &mut self.context.core.od,
            current_time_us,
        ) {
            Ok(response_data) => {
                // 5. Build and return the UDP response action
                match self
                    .context
                    .udp_transport
                    .build_response(response_data, &self.context)
                {
                    Ok(action) => action,
                    Err(e) => {
                        error!("[MN] Failed to build SDO/UDP response: {:?}", e);
                        NodeAction::NoAction
                    }
                }
            }
            Err(e) => {
                error!("[MN] SDO server error (UDP): {:?}", e);
                NodeAction::NoAction
            }
        }
    }

    /// The MN's main scheduler tick.
    fn tick(&mut self, current_time_us: u64) -> NodeAction {
        // Delegate almost everything to the scheduler
        scheduler::tick(&mut self.context, current_time_us)
    }
}

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
                    return action;
                }
            }
            // Ignore non-POWERLINK Ethernet frames
        }

        // --- Priority 2: UDP Datagrams ---
        if let Some((buffer, ip, port)) = udp_datagram {
            let action = self.process_udp_datagram(buffer, ip, port, current_time_us);
            if action != NodeAction::NoAction {
                return action;
            }
        }

        // --- Priority 3: Internal Ticks ---
        self.tick(current_time_us)
    }

    #[cfg(not(feature = "sdo-udp"))]
    fn run_cycle(
        &mut self,
        ethernet_frame: Option<&[u8]>,
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
                    return action;
                }
            }
            // Ignore non-POWERLINK Ethernet frames
        }

        // --- Priority 2: Internal Ticks ---
        self.tick(current_time_us)
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
            return Some(0);
        }
        self.context.next_tick_us
    }
}