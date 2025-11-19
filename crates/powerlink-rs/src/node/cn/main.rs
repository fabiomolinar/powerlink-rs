use super::events;
use super::state::CnContext;
use crate::PowerlinkError;
use crate::frame::basic::MacAddress;
use crate::frame::error::{CnErrorCounters, DllErrorManager, LoggingErrorHandler};
use crate::frame::{DllError, NmtAction, ServiceId, deserialize_frame};
use crate::nmt::cn_state_machine::CnNmtStateMachine;
use crate::nmt::events::NmtEvent;
use crate::nmt::events::{CnNmtRequest, NmtStateCommand};
use crate::nmt::state_machine::NmtStateMachine;
use crate::nmt::states::NmtState;
use crate::node::{CoreNodeContext, Node, NodeAction};
use crate::od::{Object, ObjectDictionary, ObjectValue, constants};
use crate::sdo::transport::AsndTransport;
#[cfg(feature = "sdo-udp")]
use crate::sdo::transport::UdpTransport;
use crate::sdo::{EmbeddedSdoClient, EmbeddedSdoServer, SdoClient, SdoServer};
#[cfg(feature = "sdo-udp")]
use crate::sdo::{
    server::SdoClientInfo, transport::SdoTransport, udp::deserialize_sdo_udp_payload,
};
#[cfg(feature = "sdo-udp")]
use crate::types::IpAddress;
use crate::types::{C_ADR_MN_DEF_NODE_ID, MessageType, NodeId};
use alloc::collections::{BTreeMap, VecDeque};
use alloc::vec::Vec;
#[cfg(feature = "sdo-udp")]
use log::debug;
use log::{error, info, warn};

/// Represents a complete POWERLINK Controlled Node (CN).
/// This struct is a thin wrapper around a context object that holds all state.
pub struct ControlledNode<'s> {
    pub context: CnContext<'s>,
}

impl<'s> ControlledNode<'s> {
    /// Creates a new Controlled Node.
    ///
    /// The application is responsible for creating and populating the Object Dictionary
    /// with device-specific parameters (e.g., Identity Object 0x1018) before passing
    /// it to this constructor. This function will then read the necessary configuration
    /// from the OD to initialize the NMT state machine.
    pub fn new(
        mut od: ObjectDictionary<'s>,
        mac_address: MacAddress,
    ) -> Result<Self, PowerlinkError> {
        info!("Creating new Controlled Node.");
        // Initialise the OD, which involves loading from storage or applying defaults.
        od.init()?;

        // Validate that the user-provided OD contains all mandatory objects.
        od.validate_mandatory_objects(false)?; // false for CN validation

        // The NMT state machine's constructor is now fallible because it must
        // read critical parameters from the fully configured OD.
        let nmt_state_machine = CnNmtStateMachine::from_od(&od)?;

        // --- Parse Heartbeat Configuration (OD 0x1016) ---
        let mut heartbeat_consumers = BTreeMap::new();
        if let Some(Object::Array(entries)) =
            od.read_object(constants::IDX_NMT_CONSUMER_HEARTBEAT_TIME_AU32)
        {
            // Sub-index 0 is NumberOfEntries, actual entries start at sub-index 1.
            // The `entries` Vec maps to sub-indices 1..N.
            for hb_value in entries {
                if let ObjectValue::Unsigned32(hb_description) = hb_value {
                    // Spec 7.2.1.5.4, Table 120: HeartbeatDescription
                    let heartbeat_time_ms = (hb_description & 0xFFFF) as u16;
                    let node_id_val = ((hb_description >> 16) & 0xFF) as u8;

                    if heartbeat_time_ms > 0 && node_id_val > 0 {
                        if let Ok(node_id) = NodeId::try_from(node_id_val) {
                            let timeout_us = (heartbeat_time_ms as u64) * 1000;
                            // Initialize last_seen_us to 0. It will be set on the first tick or frame.
                            heartbeat_consumers.insert(node_id, (timeout_us, 0));
                            info!(
                                "[CN] Added heartbeat consumer: Node {} with timeout {}ms",
                                node_id.0, heartbeat_time_ms
                            );
                        } else {
                            warn!(
                                "[CN] Invalid Node ID {} in heartbeat configuration (0x1016).",
                                node_id_val
                            );
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
            embedded_sdo_server: EmbeddedSdoServer::new(),
            embedded_sdo_client: EmbeddedSdoClient::new(),
        };

        let mut node = Self {
            context: CnContext {
                core: core_context, // Use the new core context
                nmt_state_machine,
                dll_state_machine: Default::default(),
                dll_error_manager: DllErrorManager::new(
                    CnErrorCounters::new(),
                    LoggingErrorHandler,
                ),
                asnd_transport: AsndTransport,
                #[cfg(feature = "sdo-udp")]
                udp_transport: UdpTransport,
                pending_nmt_requests: Vec::new(),
                emergency_queue: VecDeque::with_capacity(10), // Default capacity for 10 errors
                heartbeat_consumers,                          // Add the new map
                last_soc_reception_time_us: 0,
                soc_timeout_check_active: false,
                next_tick_us: None,
                en_flag: false,
                // Per spec 6.5.5.1, EC starts as 1 to indicate "not initialized"
                ec_flag: true,
                error_status_changed: false,
            },
        };

        // Run the initial state transitions to get to NmtNotActive.
        node.context
            .nmt_state_machine
            .run_internal_initialisation(&mut node.context.core.od); // Access OD through core

        Ok(node)
    }

    /// Allows the application to queue an SDO request payload to be sent.
    pub fn queue_sdo_request(&mut self, payload: Vec<u8>) {
        self.context
            .core
            .queue_sdo_request(NodeId(C_ADR_MN_DEF_NODE_ID), payload);
    }

    /// Allows the application to queue an NMT state command request to be sent to the MN.
    /// (Reference: EPSG DS 301, Section 7.3.6)
    pub fn queue_nmt_request(&mut self, command: NmtStateCommand, target: NodeId) {
        info!(
            "Queueing NMT State Command request: Command={:?}, Target={}",
            command, target.0
        );
        self.context
            .pending_nmt_requests
            .push((CnNmtRequest::Command(command), target));
    }

    /// Processes a POWERLINK Ethernet frame.
    fn process_ethernet_frame(&mut self, buffer: &[u8], current_time_us: u64) -> NodeAction {
        // Check if we are in BasicEthernet
        if self.nmt_state() == NmtState::NmtBasicEthernet {
            info!(
                "[CN] POWERLINK frame detected in NmtBasicEthernet. Transitioning to NmtPreOperational1."
            );
            // Trigger the NMT transition
            self.context
                .nmt_state_machine
                .process_event(NmtEvent::PowerlinkFrameReceived, &mut self.context.core.od);
            // Fall through to process the frame that triggered the transition
        }

        // --- Peek for ASnd SDO Rx ---
        // We peek here to increment the counter before passing it to the full
        // deserializer and SDO server.
        // We already know EtherType is 0x88AB from run_cycle.
        // Check for ASnd (0x06), Dest=Self, and SDO Service (0x05)
        if buffer.len() > 17 && // 14 (EthHdr) + 3 (PLHdr) + 1 (SvcID)
           buffer[14] == MessageType::ASnd as u8 && // MessageType
           buffer[15] == self.context.nmt_state_machine.node_id.0 && // Dest Node ID
           buffer[17] == ServiceId::Sdo as u8
        // ServiceID
        {
            self.context.core.od.increment_counter(
                constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
                constants::SUBIDX_DIAG_NMT_COUNT_SDO_RX,
            );
        }
        // --- END PEEK ---

        match deserialize_frame(buffer) {
            Ok(frame) => events::process_frame(&mut self.context, frame, current_time_us),
            Err(e) if e != PowerlinkError::InvalidEthernetFrame => {
                // Looked like POWERLINK (correct EtherType) but malformed. Log as warning.
                warn!(
                    "[CN] Could not deserialize potential POWERLINK frame: {:?} (Buffer len: {})",
                    e,
                    buffer.len()
                );
                // Report as InvalidFormat DLL error
                let (nmt_action, signaled) = self
                    .context
                    .dll_error_manager
                    .handle_error(DllError::InvalidFormat);
                if signaled {
                    self.context.error_status_changed = true;
                    // Update Error Register (0x1001), Set Bit 0: Generic Error
                    let current_err_reg = self
                        .context
                        .core
                        .od
                        .read_u8(constants::IDX_NMT_ERROR_REGISTER_U8, 0)
                        .unwrap_or(0);
                    let new_err_reg = current_err_reg | 0b1;
                    self.context
                        .core
                        .od
                        .write_internal(
                            constants::IDX_NMT_ERROR_REGISTER_U8,
                            0,
                            crate::od::ObjectValue::Unsigned8(new_err_reg),
                            false,
                        )
                        .unwrap_or_else(|e| {
                            error!("[CN] Failed to update Error Register: {:?}", e)
                        });
                }
                // Trigger NMT error handling if required
                if nmt_action != NmtAction::None {
                    self.context
                        .nmt_state_machine
                        .process_event(NmtEvent::Error, &mut self.context.core.od);
                }
                NodeAction::NoAction
            }
            _ => NodeAction::NoAction, // Ignore other EtherTypes silently
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
            "[CN] Received UDP datagram ({} bytes) from {}:{}",
            buffer.len(),
            core::net::Ipv4Addr::from(source_ip),
            source_port
        );

        // 1. Deserialize the SDO payload from the UDP datagram
        let (seq_header, cmd) = match deserialize_sdo_udp_payload(buffer) {
            Ok((seq, cmd)) => (seq, cmd),
            Err(e) => {
                warn!("[CN] Failed to deserialize SDO/UDP payload: {:?}", e);
                // Cannot send a response if we can't parse the request
                return NodeAction::NoAction;
            }
        };

        // *** INCREMENT SDO RX COUNTER ***
        self.context.core.od.increment_counter(
            constants::IDX_DIAG_NMT_TELEGR_COUNT_REC,
            constants::SUBIDX_DIAG_NMT_COUNT_SDO_RX,
        );

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
                        error!("[CN] Failed to build SDO/UDP response: {:?}", e);
                        NodeAction::NoAction
                    }
                }
            }
            Err(e) => {
                error!("[CN] SDO server error (UDP): {:?}", e);
                NodeAction::NoAction
            }
        }
    }

    /// Internal tick handler, moved from the trait implementation.
    fn tick(&mut self, current_time_us: u64) -> NodeAction {
        events::process_tick(&mut self.context, current_time_us)
    }
}

impl<'s> Node for ControlledNode<'s> {
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
                    // SDO Tx counter (for ASnd) is handled inside events::process_frame
                    return action;
                }
            }
            // Ignore non-POWERLINK Ethernet frames
        }

        // --- Priority 2: UDP Datagrams ---
        if let Some((buffer, ip, port)) = udp_datagram {
            let action = self.process_udp_datagram(buffer, ip, port, current_time_us);
            if let NodeAction::SendUdp { .. } = action {
                // Increment SDO Tx counter for UDP response
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
        self.tick(current_time_us)
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
                    // SDO Tx counter (for ASnd) is handled inside events::process_frame
                    return action;
                }
            }
            // Ignore non-POWERLINK Ethernet frames
        }

        // --- Priority 2: Internal Ticks ---
        self.tick(current_time_us)
    }

    fn nmt_state(&self) -> NmtState {
        self.context.nmt_state_machine.current_state()
    }

    fn next_action_time(&self) -> Option<u64> {
        self.context.next_tick_us
    }
}
