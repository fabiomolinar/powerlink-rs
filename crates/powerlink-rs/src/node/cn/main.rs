use super::payload;
use crate::frame::ASndFrame;
use crate::sdo::asnd;
use crate::PowerlinkError;
use crate::common::NetTime;
use crate::frame::{
    DllCsEvent,
    DllCsStateMachine,
    DllError,
    NmtAction,
    PReqFrame,
    PResFrame,
    PowerlinkFrame,
    RequestedServiceId,
    ServiceId,    
    basic::{EthernetHeader, MacAddress},
    deserialize_frame,
    error::{
        CnErrorCounters, DllErrorManager, EntryType, ErrorCounters, ErrorEntry, ErrorEntryMode,
        ErrorHandler, LoggingErrorHandler,
    },
};
use crate::frame::codec::CodecHelpers;
use crate::nmt::cn_state_machine::CnNmtStateMachine;
use crate::nmt::events::{NmtCommand, NmtEvent};
use crate::nmt::state_machine::NmtStateMachine;
use crate::nmt::states::NmtState;
use crate::node::{Node, NodeAction, PdoHandler};
use crate::od::ObjectDictionary;
#[cfg(feature = "sdo-udp")]
use crate::sdo::udp::{deserialize_sdo_udp_payload, serialize_sdo_udp_payload};
use crate::sdo::server::{SdoClientInfo, SdoResponseData};
use crate::sdo::SdoServer;
use crate::types::{C_ADR_MN_DEF_NODE_ID, MessageType, NodeId};
use alloc::collections::VecDeque;
use alloc::vec;
use alloc::vec::Vec;
use log::{debug, error, info, trace, warn};

// Constants for OD access
const OD_IDX_CYCLE_TIME: u16 = 0x1006;
const OD_IDX_LOSS_SOC_TOLERANCE: u16 = 0x1C14;
const OD_IDX_ERROR_REGISTER: u16 = 0x1001;

/// Represents a complete POWERLINK Controlled Node (CN).
/// This struct owns and manages all protocol layers and state machines.
pub struct ControlledNode<'s> {
    pub od: ObjectDictionary<'s>,
    pub(super) nmt_state_machine: CnNmtStateMachine,
    dll_state_machine: DllCsStateMachine,
    dll_error_manager: DllErrorManager<CnErrorCounters, LoggingErrorHandler>,
    mac_address: MacAddress,
    sdo_server: SdoServer,
    /// Queue for NMT commands this CN wants the MN to execute.
    pending_nmt_requests: Vec<(NmtCommand, NodeId)>,
    /// Queue for detailed error/event entries to be reported in StatusResponse.
    emergency_queue: VecDeque<ErrorEntry>,
    /// Timestamp of the last successfully received SoC frame (microseconds).
    last_soc_reception_time_us: u64,
    /// Flag indicating if the SoC timeout check is currently active.
    soc_timeout_check_active: bool,
    /// The absolute time in microseconds for the next scheduled tick.
    next_tick_us: Option<u64>,
    /// Exception New flag, toggled when new error info is available.
    en_flag: bool,
    /// Exception Clear flag, mirrors the last received ER flag from the MN.
    ec_flag: bool,
    /// A flag that is set when a new error occurs, to trigger toggling the EN flag.
    error_status_changed: bool,
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

        let mut node = Self {
            od,
            nmt_state_machine,
            dll_state_machine: DllCsStateMachine::new(),
            dll_error_manager: DllErrorManager::new(CnErrorCounters::new(), LoggingErrorHandler),
            mac_address,
            sdo_server: SdoServer::new(),
            pending_nmt_requests: Vec::new(),
            emergency_queue: VecDeque::with_capacity(10), // Default capacity for 10 errors
            last_soc_reception_time_us: 0,
            soc_timeout_check_active: false,
            next_tick_us: None,
            en_flag: false,
            // Per spec 6.5.5.1, EC starts as 1 to indicate "not initialized"
            ec_flag: true,
            error_status_changed: false,
        };

        // Run the initial state transitions to get to NmtNotActive.
        node.nmt_state_machine
            .run_internal_initialisation(&mut node.od);

        Ok(node)
    }

    /// Allows the application to queue an SDO request payload to be sent.
    pub fn queue_sdo_request(&mut self, payload: Vec<u8>) {
        self.sdo_server.queue_request(payload);
    }

    /// Allows the application to queue an NMT command request to be sent to the MN.
    /// (Reference: EPSG DS 301, Section 7.3.6)
    pub fn queue_nmt_request(&mut self, command: NmtCommand, target: NodeId) {
        info!(
            "Queueing NMT request: Command={:?}, Target={}",
            command, target.0
        );
        self.pending_nmt_requests.push((command, target));
    }

    /// Processes a POWERLINK Ethernet frame.
    fn process_ethernet_frame(
        &mut self,
        buffer: &[u8],
        current_time_us: u64,
    ) -> NodeAction {
        match deserialize_frame(buffer) {
            Ok(frame) => self.process_frame(frame, current_time_us),
            Err(e) if e != PowerlinkError::InvalidEthernetFrame => {
                // Looked like POWERLINK (correct EtherType) but malformed. Log as warning.
                warn!(
                    "[CN] Could not deserialize potential POWERLINK frame: {:?} (Buffer len: {})",
                    e, buffer.len()
                );
                 // Report as InvalidFormat DLL error
                let (nmt_action, signaled) =
                    self.dll_error_manager.handle_error(DllError::InvalidFormat);
                if signaled {
                    self.error_status_changed = true;
                     // Update Error Register (0x1001), Set Bit 0: Generic Error
                    let current_err_reg = self.od.read_u8(OD_IDX_ERROR_REGISTER, 0).unwrap_or(0);
                    let new_err_reg = current_err_reg | 0b1;
                    self.od.write_internal(OD_IDX_ERROR_REGISTER, 0, crate::od::ObjectValue::Unsigned8(new_err_reg), false)
                       .unwrap_or_else(|e| error!("[CN] Failed to update Error Register: {:?}", e));
                }
                 // Trigger NMT error handling if required
                if nmt_action != NmtAction::None {
                    self.nmt_state_machine.process_event(NmtEvent::Error, &mut self.od);
                }
                NodeAction::NoAction
            }
            _ => NodeAction::NoAction, // Ignore other EtherTypes silently
        }
    }

    /// Processes an SDO request received over UDP.
    #[cfg(feature = "sdo-udp")]
    fn process_udp_packet(
        &mut self,
        data: &[u8],
        source_ip: crate::types::IpAddress,
        source_port: u16,
        current_time_us: u64,
    ) -> NodeAction {
        debug!(
            "Received UDP SDO request from {}:{} ({} bytes)",
            core::net::Ipv4Addr::from(source_ip),
            source_port,
            data.len()
        );

        // Validate UDP SDO prefix (from EPSG DS 301, Table 47) and get the SDO payload slice.
        // The SDO payload starts *after* the 4-byte POWERLINK UDP prefix.
        let sdo_payload = match data {
            // Check for prefix: MessageType(ASnd), Reserved(2), ServiceID(Sdo)
            [0x06, _, _, 0x05, rest @ ..] => rest,
            _ => {
                error!("Invalid or malformed SDO/UDP payload prefix.");
                // We cannot send an SDO abort because the frame is fundamentally broken.
                return NodeAction::NoAction;
            }
        };

        let client_info = SdoClientInfo::Udp {
            source_ip,
            source_port,
        };
        match self
            .sdo_server
            .handle_request(sdo_payload, client_info, &mut self.od, current_time_us)
        {
            Ok(response_data) => match self.build_udp_from_sdo_response(response_data) {
                Ok(action) => action,
                Err(e) => {
                    error!("Failed to build SDO/UDP response: {:?}", e);
                    NodeAction::NoAction
                }
            },
            Err(e) => {
                error!("SDO server error (UDP): {:?}", e);
                // Abort is handled internally by SdoServer::handle_request now
                NodeAction::NoAction
            }
        }
    }

    /// Internal function to process a deserialized `PowerlinkFrame`.
    fn process_frame(&mut self, frame: PowerlinkFrame, current_time_us: u64) -> NodeAction {
        // --- Special handling for SDO ASnd frames ---
        if let PowerlinkFrame::ASnd(ref asnd_frame) = frame {
             if asnd_frame.destination == self.nmt_state_machine.node_id && asnd_frame.service_id == ServiceId::Sdo {
                debug!("Received SDO/ASnd frame for processing.");
                // SDO payload starts right after ASnd header (MType, Dest, Src, SvcID = 4 bytes)
                 let sdo_payload = &asnd_frame.payload; // ASnd payload *is* the SDO payload (Seq+Cmd+Data)
                 let client_info = SdoClientInfo::Asnd {
                    source_node_id: asnd_frame.source,
                    source_mac: asnd_frame.eth_header.source_mac,
                };

                 match self.sdo_server.handle_request(
                    sdo_payload,
                    client_info,
                    &mut self.od,
                    current_time_us,
                ) {
                    Ok(response_data) => {
                         match self.build_asnd_from_sdo_response(response_data) {
                             Ok(action) => return action,
                             Err(e) => {
                                 error!("Failed to build SDO/ASnd response: {:?}", e);
                                 return NodeAction::NoAction;
                             }
                         }
                    }
                    Err(e) => {
                        error!("SDO server error (ASnd): {:?}", e);
                        // Abort handled internally
                         return NodeAction::NoAction;
                    }
                };
            } else if asnd_frame.destination == self.nmt_state_machine.node_id {
                 trace!("Received non-SDO ASnd frame: {:?}", asnd_frame);
             } else {
                 return NodeAction::NoAction; // ASnd not for us
             }
        }

        // --- Handle SoC Frame specific logic ---
        if let PowerlinkFrame::Soc(_) = &frame {
            trace!("SoC received at time {}", current_time_us);
            self.last_soc_reception_time_us = current_time_us;
            self.soc_timeout_check_active = true;
            if self.dll_error_manager.on_cycle_complete() {
                info!("[CN] All DLL errors cleared, resetting Generic Error bit.");
                let current_err_reg = self.od.read_u8(OD_IDX_ERROR_REGISTER, 0).unwrap_or(0);
                let new_err_reg = current_err_reg & !0b1;
                self.od.write_internal(
                    OD_IDX_ERROR_REGISTER, 0, crate::od::ObjectValue::Unsigned8(new_err_reg), false
                ).unwrap_or_else(|e| error!("[CN] Failed to clear Error Register: {:?}", e));
                self.error_status_changed = true;
            }

            let cycle_time_opt = self.od.read_u32(OD_IDX_CYCLE_TIME, 0).map(|v| v as u64);
            let tolerance_opt = self.od.read_u32(OD_IDX_LOSS_SOC_TOLERANCE, 0).map(|v| v as u64);

            if let (Some(cycle_time_us), Some(tolerance_ns)) = (cycle_time_opt, tolerance_opt) {
                if cycle_time_us > 0 {
                    let tolerance_us = tolerance_ns / 1000;
                    let deadline = current_time_us + cycle_time_us + tolerance_us;
                     match self.next_tick_us {
                        Some(current_deadline) if deadline < current_deadline => {
                            self.next_tick_us = Some(deadline);
                            trace!("Scheduled SoC timeout check at {}us (earlier)", deadline);
                        }
                        None => {
                            self.next_tick_us = Some(deadline);
                             trace!("Scheduled SoC timeout check at {}us (first)", deadline);
                        }
                         _ => {}
                    }
                } else {
                     warn!("Cycle Time (0x1006) is 0, cannot schedule SoC timeout.");
                     self.soc_timeout_check_active = false;
                }
            } else {
                warn!("Could not read Cycle Time (0x1006) or SoC Tolerance (0x1C14) from OD. SoC timeout check disabled.");
                self.soc_timeout_check_active = false;
            }
        }

        // --- Handle EA/ER flags ---
        let target_node_id_opt = match &frame {
            PowerlinkFrame::PReq(preq) => Some(preq.destination),
            PowerlinkFrame::SoA(soa) => Some(soa.target_node_id),
            _ => None,
        };
         let is_relevant_target = target_node_id_opt == Some(self.nmt_state_machine.node_id)
            || (matches!(frame, PowerlinkFrame::SoA(_)) && target_node_id_opt == Some(NodeId(crate::types::C_ADR_BROADCAST_NODE_ID)));

         if is_relevant_target {
             match &frame {
                PowerlinkFrame::PReq(preq) => {
                     if preq.destination == self.nmt_state_machine.node_id {
                        if preq.flags.ea == self.en_flag {
                             trace!("Received matching EA flag ({}) from MN in PReq.", preq.flags.ea);
                        } else {
                            trace!("Received mismatched EA flag ({}, EN is {}) from MN in PReq.", preq.flags.ea, self.en_flag);
                        }
                    }
                }
                PowerlinkFrame::SoA(soa) => {
                     if soa.target_node_id == self.nmt_state_machine.node_id {
                        if soa.flags.er {
                            info!("Received ER flag from MN in SoA, resetting EN flag and Emergency Queue.");
                            self.en_flag = false;
                            self.emergency_queue.clear();
                        }
                        self.ec_flag = soa.flags.er;
                        trace!("Processed SoA flags: ER={}, EC set to {}", soa.flags.er, self.ec_flag);
                         if soa.flags.ea == self.en_flag {
                             trace!("Received matching EA flag ({}) from MN in SoA.", soa.flags.ea);
                         } else {
                             trace!("Received mismatched EA flag ({}, EN is {}) from MN in SoA.", soa.flags.ea, self.en_flag);
                         }
                    }
                }
                _ => {}
            }
        }

        // --- Normal Frame Processing ---
        let nmt_event = match &frame {
            PowerlinkFrame::Soc(_) => Some(NmtEvent::SocReceived),
            PowerlinkFrame::SoA(_) => Some(NmtEvent::SocSoAReceived),
             PowerlinkFrame::ASnd(asnd) if asnd.destination == self.nmt_state_machine.node_id && asnd.service_id == ServiceId::NmtCommand => {
                asnd.payload.get(0).and_then(|&b| NmtCommand::try_from(b).ok()).map(|cmd| match cmd {
                    NmtCommand::StartNode => NmtEvent::StartNode,
                    NmtCommand::StopNode => NmtEvent::StopNode,
                    NmtCommand::EnterPreOperational2 => NmtEvent::EnterPreOperational2,
                    NmtCommand::EnableReadyToOperate => NmtEvent::EnableReadyToOperate,
                    NmtCommand::ResetNode => NmtEvent::ResetNode,
                    NmtCommand::ResetCommunication => NmtEvent::ResetCommunication,
                    NmtCommand::ResetConfiguration => NmtEvent::ResetConfiguration,
                    NmtCommand::SwReset => NmtEvent::SwReset,
                })
            }
            _ => None,
        };
        if let Some(event) = nmt_event {
            self.nmt_state_machine.process_event(event, &mut self.od);
        }

         let dll_event = frame.dll_cn_event();
         if let Some(errors) = self
            .dll_state_machine
            .process_event(dll_event, self.nmt_state_machine.current_state())
        {
            for error in errors {
                warn!("DLL state machine reported error: {:?}", error);
                let (nmt_action, signaled) = self.dll_error_manager.handle_error(error);
                if signaled {
                    self.error_status_changed = true;
                    let current_err_reg = self.od.read_u8(OD_IDX_ERROR_REGISTER, 0).unwrap_or(0);
                    let new_err_reg = current_err_reg | 0b1;
                    self.od.write_internal(
                        OD_IDX_ERROR_REGISTER, 0, crate::od::ObjectValue::Unsigned8(new_err_reg), false
                    ).unwrap_or_else(|e| error!("[CN] Failed to update Error Register: {:?}", e));

                    let error_entry = ErrorEntry {
                        entry_type: EntryType { is_status_entry: false, send_to_queue: true, mode: ErrorEntryMode::EventOccurred, profile: 0x002 },
                        error_code: error.to_error_code(),
                        timestamp: NetTime { seconds: (current_time_us / 1_000_000) as u32, nanoseconds: ((current_time_us % 1_000_000) * 1000) as u32 },
                        additional_information: match error { DllError::LossOfPres { node_id } | DllError::LatePres { node_id } | DllError::LossOfStatusRes { node_id } => node_id.0 as u64, _ => 0 },
                    };
                    if self.emergency_queue.len() < self.emergency_queue.capacity() {
                        self.emergency_queue.push_back(error_entry);
                        info!("[CN] New error queued: {:?}", error_entry);
                    } else {
                        warn!("[CN] Emergency queue full, dropping error: {:?}", error_entry);
                    }
                }
                 if nmt_action != NmtAction::None {
                     info!("DLL error triggered NMT action: {:?}", nmt_action);
                    self.nmt_state_machine.process_event(NmtEvent::Error, &mut self.od);
                     self.soc_timeout_check_active = false;
                      return NodeAction::NoAction; // Skip response if reset
                 }
            }
        }

        // --- PDO Consumption ---
         let is_target_or_broadcast_pdo = match &frame {
             PowerlinkFrame::PReq(f) => f.destination == self.nmt_state_machine.node_id,
             PowerlinkFrame::PRes(_) => true,
             _ => false,
        };
         if is_target_or_broadcast_pdo {
            match &frame {
                PowerlinkFrame::PReq(preq_frame) => {
                     if preq_frame.destination == self.nmt_state_machine.node_id {
                         self.consume_preq_payload(preq_frame);
                     }
                 }
                PowerlinkFrame::PRes(pres_frame) => self.consume_pres_payload(pres_frame),
                _ => {}
            }
        }

        // --- Error Signaling Flag Toggle ---
        if self.error_status_changed {
            self.en_flag = !self.en_flag;
            self.error_status_changed = false;
            info!("New error detected or acknowledged, toggling EN flag to: {}", self.en_flag);
        }

        // --- Generate Response ---
         let current_nmt_state = self.nmt_state();
         let response_frame_opt = if current_nmt_state >= NmtState::NmtNotActive {
             match &frame {
                PowerlinkFrame::SoA(soa_frame) => {
                    if soa_frame.target_node_id == self.nmt_state_machine.node_id {
                        match current_nmt_state {
                             NmtState::NmtPreOperational1 | NmtState::NmtPreOperational2
                             | NmtState::NmtReadyToOperate | NmtState::NmtOperational
                             | NmtState::NmtCsStopped => {
                                match soa_frame.req_service_id {
                                    RequestedServiceId::IdentRequest => Some(payload::build_ident_response(self.mac_address, self.nmt_state_machine.node_id, &self.od, soa_frame)),
                                    RequestedServiceId::StatusRequest => Some(payload::build_status_response(self.mac_address, self.nmt_state_machine.node_id, &mut self.od, self.en_flag, self.ec_flag, &mut self.emergency_queue, soa_frame)),
                                    RequestedServiceId::NmtRequestInvite => self.pending_nmt_requests.pop().map(|(cmd, tgt)| payload::build_nmt_request(self.mac_address, self.nmt_state_machine.node_id, cmd, tgt, soa_frame)),
                                    RequestedServiceId::UnspecifiedInvite => self.sdo_server.pop_pending_request().map(|sdo_payload| PowerlinkFrame::ASnd(ASndFrame::new(self.mac_address, soa_frame.eth_header.source_mac, NodeId(C_ADR_MN_DEF_NODE_ID), self.nmt_state_machine.node_id, ServiceId::Sdo, sdo_payload))),
                                    RequestedServiceId::NoService => None,
                                }
                            }
                            _ => None,
                        }
                    } else { None }
                }
                PowerlinkFrame::PReq(preq_frame) => {
                     if preq_frame.destination == self.nmt_state_machine.node_id {
                        match current_nmt_state {
                            NmtState::NmtPreOperational2 | NmtState::NmtReadyToOperate | NmtState::NmtOperational => Some(payload::build_pres_response(self.mac_address, self.nmt_state_machine.node_id, current_nmt_state, &self.od, &self.sdo_server, &self.pending_nmt_requests, self.en_flag)),
                            _ => None,
                        }
                    } else { None }
                }
                _ => None,
            }
        } else { None };

        // --- Serialize and return action ---
        if let Some(response_frame) = response_frame_opt {
             match self.serialize_and_prepare_action(response_frame) {
                Ok(action) => return action,
                Err(e) => {
                     error!("Failed to prepare response action: {:?}", e);
                     return NodeAction::NoAction;
                }
            }
        }

        NodeAction::NoAction
    }

    /// Helper to build ASnd frame from SdoResponseData.
    fn build_asnd_from_sdo_response(
        &self,
        response_data: SdoResponseData,
    ) -> Result<NodeAction, PowerlinkError> {
        let (source_node_id, source_mac) = match response_data.client_info {
            SdoClientInfo::Asnd {
                source_node_id,
                source_mac,
            } => (source_node_id, source_mac),
            #[cfg(feature = "sdo-udp")]
            SdoClientInfo::Udp { .. } => {
                return Err(PowerlinkError::InternalError(
                    "Attempted to build ASnd response for UDP client",
                ))
            }
        };

        let sdo_payload =
            asnd::serialize_sdo_asnd_payload(response_data.seq_header, response_data.command)?;
        let asnd_frame = ASndFrame::new(
            self.mac_address,
            source_mac,
            source_node_id,
            self.nmt_state_machine.node_id,
            ServiceId::Sdo,
            sdo_payload,
        );
        info!("Sending SDO response via ASnd to Node {}", source_node_id.0);
        self.serialize_and_prepare_action(PowerlinkFrame::ASnd(asnd_frame))
    }

    /// Helper to build NodeAction::SendUdp from SdoResponseData.
    #[cfg(feature = "sdo-udp")]
    fn build_udp_from_sdo_response(
        &self,
        response_data: SdoResponseData,
    ) -> Result<NodeAction, PowerlinkError> {
         let (source_ip, source_port) = match response_data.client_info {
            SdoClientInfo::Udp { source_ip, source_port } => (source_ip, source_port),
             SdoClientInfo::Asnd { .. } => return Err(PowerlinkError::InternalError("Attempted to build UDP response for ASnd client")),
        };

         let mut udp_buffer = vec![0u8; 1500]; // MTU size
         let udp_payload_len = serialize_sdo_udp_payload(
            response_data.seq_header,
            response_data.command,
            &mut udp_buffer,
        )?;
        udp_buffer.truncate(udp_payload_len);
        info!(
            "Sending SDO response via UDP to {}:{}",
            core::net::Ipv4Addr::from(source_ip), source_port
        );
        Ok(NodeAction::SendUdp {
            dest_ip: source_ip,
            dest_port: source_port,
            data: udp_buffer,
        })
    }

    /// Helper to serialize a PowerlinkFrame (Ethernet) and prepare the NodeAction.
    /// Returns Result to propagate serialization errors.
    fn serialize_and_prepare_action(&self, frame: PowerlinkFrame) -> Result<NodeAction, PowerlinkError> {
        // Estimate max size needed (14 Eth + Max PL size ~1500)
        let mut buf = vec![0u8; 1518];
        // Serialize Eth header first
        let eth_header = match &frame {
            PowerlinkFrame::PRes(f) => &f.eth_header,
            PowerlinkFrame::ASnd(f) => &f.eth_header,
            // Add other frame types if CN might send them (unlikely for responses)
            _ => {
                error!("[CN] Attempted to serialize unexpected response frame type: {:?}", frame);
                return Ok(NodeAction::NoAction); // Return NoAction on unexpected type
            }
        };
        CodecHelpers::serialize_eth_header(eth_header, &mut buf);

        // Then serialize PL part into the buffer starting after the Eth header
        match frame.serialize(&mut buf[14..]) {
            Ok(pl_size) => {
                let total_size = 14 + pl_size;
                 if total_size < 60 { // Ethernet minimum frame size (excluding preamble, SFD, FCS)
                    // Spec requires padding, but the raw socket likely handles this.
                    // We truncate to the *actual* data size.
                    buf.truncate(total_size);
                    trace!("Frame size {} bytes (padding likely handled by OS/hardware).", total_size);
                 } else {
                    buf.truncate(total_size);
                 }
                info!("Sending response frame type: {:?} ({} bytes)", frame, buf.len());
                trace!("Sending frame bytes ({}): {:02X?}", buf.len(), &buf);
                Ok(NodeAction::SendFrame(buf))
            }
            Err(e) => {
                error!("[CN] Failed to serialize response frame: {:?}", e);
                 Err(e) // Propagate serialization error
            }
        }
    }

    /// Consumes the payload of a PReq frame based on RPDO mapping.
    fn consume_preq_payload(&mut self, preq: &PReqFrame) {
        // Node ID 0 is reserved for PReq source according to spec and OD usage
        self.consume_pdo_payload(
            NodeId(0),
            &preq.payload,
            preq.pdo_version,
            preq.flags.rd, // Pass the RD flag
        );
    }

    /// Consumes the payload of a PRes frame based on RPDO mapping.
    fn consume_pres_payload(&mut self, pres: &PResFrame) {
        self.consume_pdo_payload(
            pres.source, // Source Node ID of the PRes
            &pres.payload,
            pres.pdo_version,
            pres.flags.rd, // Pass the RD flag
        );
    }
}

// Implement the PdoHandler trait for ControlledNode
impl<'s> PdoHandler<'s> for ControlledNode<'s> {
    fn od(&mut self) -> &mut ObjectDictionary<'s> {
        &mut self.od
    }

    fn dll_error_manager(&mut self) -> &mut DllErrorManager<impl ErrorCounters, impl ErrorHandler> {
        &mut self.dll_error_manager
    }
}

impl<'s> Node for ControlledNode<'s> {
    /// Processes a raw byte buffer received from the network at a specific time.
    /// This now tries to interpret the buffer as either Ethernet or UDP (if enabled).
    fn process_raw_frame(&mut self, buffer: &[u8], current_time_us: u64) -> NodeAction {
        // --- Try Ethernet Frame Processing ---
        // Check length and EtherType
        if buffer.len() >= 14 && buffer[12..14] == crate::types::C_DLL_ETHERTYPE_EPL.to_be_bytes()
        {
             // Check if we are in BasicEthernet
            if self.nmt_state() == NmtState::NmtBasicEthernet {
                info!(
                    "[CN] POWERLINK frame detected in NmtBasicEthernet. Transitioning to NmtPreOperational1."
                );
                 // Trigger the NMT transition
                self.nmt_state_machine
                    .process_event(NmtEvent::PowerlinkFrameReceived, &mut self.od);
                 // Fall through to process the frame that triggered the transition
            }
            return self.process_ethernet_frame(buffer, current_time_us);
        }

        // --- Try UDP Processing (if Feature Enabled) ---
        // Note: Raw frame processing doesn't know the source IP/Port.
        // The UDP receive logic needs to be integrated into the main loop
        // via the HAL's receive_udp method. This function only handles Ethernet frames now.
        #[cfg(feature = "sdo-udp")]
        {
             // If not a POWERLINK Ethernet frame, we might assume it *could* be UDP.
             // However, without IP/Port info, we can't process it here.
             // We'll rely on the main loop calling `interface.receive_udp()` separately.
             trace!("Ignoring non-POWERLINK Ethernet frame (potential UDP?).");
        }

        // If neither Ethernet nor handled elsewhere (UDP), ignore.
        trace!("Ignoring unknown frame type or non-PL Ethernet frame.");
        NodeAction::NoAction
    }


    fn tick(&mut self, current_time_us: u64) -> NodeAction {
        // --- SDO Server Tick (handles timeouts/retransmissions) ---
        match self.sdo_server.tick(current_time_us, &self.od) {
            Ok(Some(sdo_response_data)) => {
                 // SDO server generated an abort frame, needs to be sent.
                 // Build the appropriate frame based on client_info stored in response_data.
                 #[cfg(feature = "sdo-udp")]
                 let build_udp = || self.build_udp_from_sdo_response(sdo_response_data.clone()); // Clone needed due to borrow rules
                 #[cfg(not(feature = "sdo-udp"))]
                 // Fix the type annotation for the closure when UDP is disabled
                 let build_udp = || Err::<NodeAction, PowerlinkError>(PowerlinkError::InternalError("UDP feature disabled"));

                 match sdo_response_data.client_info {
                    SdoClientInfo::Asnd { .. } => {
                        match self.build_asnd_from_sdo_response(sdo_response_data) {
                             Ok(action) => return action,
                             Err(e) => error!("[CN] Failed to build SDO Abort ASnd frame: {:?}", e),
                         }
                    }
                     #[cfg(feature = "sdo-udp")]
                     SdoClientInfo::Udp { .. } => {
                         match build_udp() {
                             Ok(action) => return action,
                             Err(e) => error!("[CN] Failed to build SDO Abort UDP frame: {:?}", e),
                         }
                     }
                 }
                 // If building the abort frame failed, fall through to other tick logic.
            }
            Err(e) => error!("[CN] SDO Server tick error: {:?}", e),
            _ => {} // No action or no error
        }

        let current_nmt_state = self.nmt_state();
         // Check if a deadline is set and if it has passed
        let deadline_passed = self.next_tick_us.map_or(false, |deadline| current_time_us >= deadline);

        // --- Handle NmtNotActive Timeout Setup ---
        if current_nmt_state == NmtState::NmtNotActive && self.next_tick_us.is_none() {
            let timeout_us = self.nmt_state_machine.basic_ethernet_timeout as u64;
            if timeout_us > 0 {
                let deadline = current_time_us + timeout_us;
                self.next_tick_us = Some(deadline);
                debug!("No SoC/SoA seen, starting BasicEthernet timeout check ({}us). Deadline: {}us", timeout_us, deadline);
            } else {
                 debug!("BasicEthernet timeout is 0, check disabled.");
             }
            return NodeAction::NoAction; // Don't act on this first call, just set the timer.
        }

        // If no deadline has passed, do nothing else this tick.
        if !deadline_passed {
            return NodeAction::NoAction;
        }

        // --- A deadline has passed ---
        trace!("Tick deadline reached at {}us (Deadline was {:?})", current_time_us, self.next_tick_us);
        self.next_tick_us = None; // Consume the deadline

        // --- Handle Specific Timeouts ---
        // NmtNotActive -> BasicEthernet
        if current_nmt_state == NmtState::NmtNotActive {
             let timeout_us = self.nmt_state_machine.basic_ethernet_timeout as u64;
             if timeout_us > 0 {
                warn!("BasicEthernet timeout expired. Transitioning state.");
                self.nmt_state_machine.process_event(NmtEvent::Timeout, &mut self.od);
                self.soc_timeout_check_active = false;
             }
             return NodeAction::NoAction; // No further action this tick
        }
        // SoC Timeout Check
        else if self.soc_timeout_check_active {
            warn!("SoC timeout detected at {}us! Last SoC was at {}us.", current_time_us, self.last_soc_reception_time_us);
            if let Some(errors) = self
                .dll_state_machine
                .process_event(DllCsEvent::SocTimeout, current_nmt_state)
            {
                for error in errors {
                    let (nmt_action, signaled) = self.dll_error_manager.handle_error(error);
                    if signaled {
                         self.error_status_changed = true;
                         // Update Error Register (0x1001)
                         let current_err_reg = self.od.read_u8(OD_IDX_ERROR_REGISTER, 0).unwrap_or(0);
                         let new_err_reg = current_err_reg | 0b1; // Set Generic Error
                         self.od.write_internal(OD_IDX_ERROR_REGISTER, 0, crate::od::ObjectValue::Unsigned8(new_err_reg), false)
                             .unwrap_or_else(|e| error!("[CN] Failed to update Error Register: {:?}", e));
                    }
                    if nmt_action != NmtAction::None {
                        self.nmt_state_machine.process_event(NmtEvent::Error, &mut self.od);
                        self.soc_timeout_check_active = false;
                         return NodeAction::NoAction; // Stop processing after NMT reset
                    }
                }
            }
             // Reschedule next check if still active
             if self.soc_timeout_check_active {
                 let cycle_time_opt = self.od.read_u32(OD_IDX_CYCLE_TIME, 0).map(|v| v as u64);
                 let tolerance_opt = self.od.read_u32(OD_IDX_LOSS_SOC_TOLERANCE, 0).map(|v| v as u64);

                if let (Some(cycle_time_us), Some(tolerance_ns)) = (cycle_time_opt, tolerance_opt) {
                     if cycle_time_us > 0 {
                         let cycles_missed = ((current_time_us - self.last_soc_reception_time_us) / cycle_time_us) + 1;
                         let next_expected_soc_time = self.last_soc_reception_time_us + cycles_missed * cycle_time_us;
                         let next_deadline = next_expected_soc_time + (tolerance_ns / 1000);
                        self.next_tick_us = Some(next_deadline);
                         trace!("SoC timeout occurred, scheduling next check at {}us", next_deadline);
                     } else { self.soc_timeout_check_active = false; }
                 } else { self.soc_timeout_check_active = false; }
            }
        } else {
             trace!("Tick deadline reached, but no specific timeout active (State: {:?}).", current_nmt_state);
        }

        NodeAction::NoAction // Default return if no frame needs sending
    }

    fn nmt_state(&self) -> NmtState {
        self.nmt_state_machine.current_state()
    }

    fn next_action_time(&self) -> Option<u64> {
        self.next_tick_us
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        nmt::flags::FeatureFlags,
        od::{AccessType, Category, Object, ObjectEntry, ObjectValue, PdoMapping},
    };

    // Helper function to create a minimal Object Dictionary for CN tests.
    fn get_test_od() -> ObjectDictionary<'static> {
        let mut od = ObjectDictionary::new(None);
        let node_id = 42u8;

        // --- Common Mandatory Objects ---
        od.insert(
            0x1000, // NMT_DeviceType_U32
            ObjectEntry {
                object: Object::Variable(ObjectValue::Unsigned32(0x000F0191)), // Example value
                name: "DeviceType",
                category: Category::Mandatory,
                access: Some(AccessType::Constant),
                default_value: None,
                value_range: None,
                pdo_mapping: None,
            },
        );
        od.insert(
            0x1018, // NMT_IdentityObject_REC
            ObjectEntry {
                object: Object::Record(vec![
                    ObjectValue::Unsigned32(1), // VendorId
                    ObjectValue::Unsigned32(2), // ProductCode
                    ObjectValue::Unsigned32(3), // RevisionNo
                    ObjectValue::Unsigned32(4), // SerialNo
                ]),
                name: "IdentityObject",
                category: Category::Mandatory,
                access: None,
                default_value: None,
                value_range: None,
                pdo_mapping: None,
            },
        );
        let flags = FeatureFlags::ISOCHRONOUS | FeatureFlags::SDO_ASND;
        od.insert(
            0x1F82, // NMT_FeatureFlags_U32
            ObjectEntry {
                object: Object::Variable(ObjectValue::Unsigned32(flags.0)),
                name: "FeatureFlags",
                category: Category::Mandatory,
                access: Some(AccessType::Constant),
                default_value: None,
                value_range: None,
                pdo_mapping: None,
            },
        );

        // --- CN Specific Mandatory Objects ---
        od.insert(
            0x1F93, // NMT_EPLNodeID_REC
            ObjectEntry {
                object: Object::Record(vec![
                    ObjectValue::Unsigned8(node_id),
                    ObjectValue::Boolean(0),
                ]),
                name: "NodeIDConfig",
                category: Category::Mandatory,
                access: None,
                default_value: None,
                value_range: None,
                pdo_mapping: None,
            },
        );
        od.insert(
            0x1F99, // NMT_CNBasicEthernetTimeout_U32
            ObjectEntry {
                object: Object::Variable(ObjectValue::Unsigned32(5_000_000)), // 5 seconds
                name: "BasicEthTimeout",
                category: Category::Mandatory,
                access: Some(AccessType::ReadWriteStore),
                default_value: None,
                value_range: None,
                pdo_mapping: None,
            },
        );

        // --- Other Objects Needed by Tests/Code ---
        od.insert(
            0x1F8C, // NMT_CurrNMTState_U8 (Used by update_od_state)
            ObjectEntry {
                object: Object::Variable(ObjectValue::Unsigned8(0)),
                name: "CurrentNMTState",
                category: Category::Mandatory, // Spec lists as Mandatory
                access: Some(AccessType::ReadOnly),
                default_value: None,
                value_range: None,
                pdo_mapping: Some(PdoMapping::No), // Spec lists mapping as No
            },
        );
        od.insert(
            0x1006, // NMT_CycleLen_U32 (Needed for SoC timeout scheduling)
            ObjectEntry {
                object: Object::Variable(ObjectValue::Unsigned32(10000)), // 10ms example
                name: "CycleLength",
                category: Category::Mandatory, // Spec lists as Mandatory
                access: Some(AccessType::ReadWriteStore),
                default_value: None,
                value_range: None,
                pdo_mapping: None,
            },
        );
        od.insert(
            0x1C14, // DLL_CNLossOfSocTolerance_U32 (Needed for SoC timeout scheduling)
            ObjectEntry {
                object: Object::Variable(ObjectValue::Unsigned32(100000)), // 100us example
                name: "LossSocTolerance",
                category: Category::Mandatory, // Spec lists as Mandatory for CN
                access: Some(AccessType::ReadWriteStore),
                default_value: None,
                value_range: None,
                pdo_mapping: None,
            },
        );
        // Add minimal PDO config objects required by payload::build_pres_response
        od.insert(
            0x1800, // TPDO Comm Param (for PRes)
            ObjectEntry {
                object: Object::Record(vec![
                    ObjectValue::Unsigned8(node_id),
                    ObjectValue::Unsigned8(0),
                ]),
                name: "TPDO1CommParam",
                category: Category::Mandatory,
                access: None,
                default_value: None,
                value_range: None,
                pdo_mapping: None,
            },
        );
        od.insert(
            0x1A00, // TPDO Mapping Param (for PRes)
            ObjectEntry {
                object: Object::Array(vec![]), // Empty mapping
                name: "TPDO1MapParam",
                category: Category::Mandatory,
                access: None,
                default_value: None,
                value_range: None,
                pdo_mapping: None,
            },
        );
        od.insert(
            0x1F98, // NMT_CycleTiming_REC (needed for PresActPayloadLimit)
            ObjectEntry {
                object: Object::Record(vec![
                    ObjectValue::Unsigned16(1490),
                    ObjectValue::Unsigned16(1490),
                    ObjectValue::Unsigned32(10000),
                    ObjectValue::Unsigned16(100),
                    ObjectValue::Unsigned16(36), // PresActPayloadLimit_U16 = 36
                    ObjectValue::Unsigned32(20000),
                    ObjectValue::Unsigned8(0),
                    ObjectValue::Unsigned16(300),
                    ObjectValue::Unsigned16(2),
                ]),
                name: "CycleTiming",
                category: Category::Mandatory,
                access: None,
                default_value: None,
                value_range: None,
                pdo_mapping: None,
            },
        );
        od.insert(
            0x1001, // ERR_ErrorRegister_U8 (used in build_status_response)
            ObjectEntry {
                object: Object::Variable(ObjectValue::Unsigned8(0)),
                name: "ErrorRegister",
                category: Category::Mandatory,
                access: Some(AccessType::ReadOnly),
                default_value: None,
                value_range: None,
                pdo_mapping: Some(PdoMapping::Optional),
            },
        );

        od
    }

    // Helper for creating a CN state machine for tests
    fn get_test_nmt() -> CnNmtStateMachine {
        let node_id = NodeId::try_from(42).unwrap();
        let feature_flags = FeatureFlags::ISOCHRONOUS | FeatureFlags::SDO_ASND;
        CnNmtStateMachine::new(node_id, feature_flags, 5_000_000)
    }

    #[test]
    fn test_from_od_reads_parameters() {
        let od = get_test_od();
        let nmt = CnNmtStateMachine::from_od(&od).unwrap();
        assert_eq!(nmt.node_id, NodeId(42));
        assert!(nmt.feature_flags.contains(FeatureFlags::SDO_ASND));
        assert_eq!(nmt.basic_ethernet_timeout, 5_000_000);
    }

    #[test]
    fn test_from_od_fails_if_missing_objects() {
        // Create an empty OD, missing mandatory objects
        let od = ObjectDictionary::new(None);
        // CnNmtStateMachine::from_od calls od.validate_mandatory_objects internally
        // Let's test ControlledNode::new directly which also calls validate
        let result = ControlledNode::new(od, MacAddress([0; 6]));
        assert!(matches!(
            result,
            Err(PowerlinkError::ValidationError(
                "Missing common mandatory object"
            ))
        ));
    }

    #[test]
    fn test_internal_boot_sequence() {
        let mut od = get_test_od(); // Use the corrected OD
        // Create the node, which runs init() and validate() internally
        let mut node = ControlledNode::new(od, MacAddress([0; 6])).unwrap();
        // The constructor already runs run_internal_initialisation.
        assert_eq!(node.nmt_state(), NmtState::NmtNotActive);
        // Verify OD state was updated
        assert_eq!(
            node.od.read_u8(0x1F8C, 0),
            Some(NmtState::NmtNotActive as u8)
        );
    }

    #[test]
    fn test_full_boot_up_happy_path() {
        let mut od = get_test_od();
        // Create node, runs init, validate, internal_init -> NmtNotActive
        let mut node = ControlledNode::new(od, MacAddress([0; 6])).unwrap();
        assert_eq!(node.nmt_state(), NmtState::NmtNotActive);

        // NMT_CT2: Receive SoA or SoC
        node.nmt_state_machine
            .process_event(NmtEvent::SocSoAReceived, &mut node.od);
        assert_eq!(node.nmt_state(), NmtState::NmtPreOperational1);

        // NMT_CT4: Receive SoC
        node.nmt_state_machine
            .process_event(NmtEvent::SocReceived, &mut node.od);
        assert_eq!(node.nmt_state(), NmtState::NmtPreOperational2);

        // NMT_CT5: Receive EnableReadyToOperate (state doesn't change yet)
        node.nmt_state_machine
            .process_event(NmtEvent::EnableReadyToOperate, &mut node.od);
        assert_eq!(node.nmt_state(), NmtState::NmtPreOperational2);

        // NMT_CT6: Application signals completion
        node.nmt_state_machine
            .process_event(NmtEvent::CnConfigurationComplete, &mut node.od);
        assert_eq!(node.nmt_state(), NmtState::NmtReadyToOperate);

        // NMT_CT7: Receive StartNode
        node.nmt_state_machine
            .process_event(NmtEvent::StartNode, &mut node.od);
        assert_eq!(node.nmt_state(), NmtState::NmtOperational);
        assert_eq!(
            node.od.read_u8(0x1F8C, 0),
            Some(NmtState::NmtOperational as u8)
        );
    }

    #[test]
    fn test_error_handling_transition() {
        let mut od = get_test_od();
        let mut node = ControlledNode::new(od, MacAddress([0; 6])).unwrap();
        // Manually set state to Operational for test
        node.nmt_state_machine.current_state = NmtState::NmtOperational;
        node.nmt_state_machine.update_od_state(&mut node.od);

        // NMT_CT11: Trigger internal error
        node.nmt_state_machine
            .process_event(NmtEvent::Error, &mut node.od);
        assert_eq!(node.nmt_state(), NmtState::NmtPreOperational1);
        assert_eq!(
            node.od.read_u8(0x1F8C, 0),
            Some(NmtState::NmtPreOperational1 as u8)
        );
    }

    #[test]
    fn test_stop_and_restart_node() {
        let mut od = get_test_od();
        let mut node = ControlledNode::new(od, MacAddress([0; 6])).unwrap();
        // Manually set state to Operational
        node.nmt_state_machine.current_state = NmtState::NmtOperational;
        node.nmt_state_machine.update_od_state(&mut node.od);

        // NMT_CT8: Receive StopNode
        node.nmt_state_machine
            .process_event(NmtEvent::StopNode, &mut node.od);
        assert_eq!(node.nmt_state(), NmtState::NmtCsStopped);
        assert_eq!(
            node.od.read_u8(0x1F8C, 0),
            Some(NmtState::NmtCsStopped as u8)
        );

        // NMT_CT10: Receive EnterPreOperational2
        node.nmt_state_machine
            .process_event(NmtEvent::EnterPreOperational2, &mut node.od);
        assert_eq!(node.nmt_state(), NmtState::NmtPreOperational2);
        assert_eq!(
            node.od.read_u8(0x1F8C, 0),
            Some(NmtState::NmtPreOperational2 as u8)
        );
    }

    #[test]
    fn test_queue_nmt_request() {
        let od = get_test_od(); // Use corrected OD setup
        // Node creation should now succeed
        let mut node = ControlledNode::new(od, MacAddress([0; 6])).unwrap();
        assert!(node.pending_nmt_requests.is_empty());

        // Queue a request
        node.queue_nmt_request(NmtCommand::ResetNode, NodeId(10));
        assert_eq!(node.pending_nmt_requests.len(), 1);
        assert_eq!(
            node.pending_nmt_requests[0],
            (NmtCommand::ResetNode, NodeId(10))
        );
    }
}