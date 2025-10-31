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

// Constants for OD access used in this file.
const OD_IDX_NODE_ASSIGNMENT: u16 = 0x1F81;
const OD_IDX_CYCLE_TIME: u16 = 0x1006;
const OD_IDX_CYCLE_TIMING_REC: u16 = 0x1F98;
const OD_SUBIDX_MULTIPLEX_CYCLE_LEN: u8 = 7;
const OD_IDX_MULTIPLEX_ASSIGN: u16 = 0x1F9B;

/// Represents a complete POWERLINK Managing Node (MN).
/// This struct is now a thin wrapper around the MnContext.
pub struct ManagingNode<'s> {
    pub(super) context: MnContext<'s>,
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
    fn process_powerlink_frame(
        &mut self,
        frame: PowerlinkFrame,
        current_time_us: u64,
    ) -> NodeAction {
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
}

impl<'s> Node for ManagingNode<'s> {
    /// Processes a raw byte buffer received from the network.
    fn process_raw_frame(&mut self, buffer: &[u8], current_time_us: u64) -> NodeAction {
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
            Ok(frame) => self.process_powerlink_frame(frame, current_time_us),
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

    /// The MN's main scheduler tick.
    fn tick(&mut self, current_time_us: u64) -> NodeAction {
        // Delegate almost everything to the scheduler
        scheduler::tick(&mut self.context, current_time_us)
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
