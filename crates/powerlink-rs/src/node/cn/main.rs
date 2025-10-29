// crates/powerlink-rs/src/node/cn/main.rs
use super::payload;
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
    basic::MacAddress,
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
use crate::sdo::SdoServer;
use crate::sdo::command::SdoCommand;
use crate::types::{C_ADR_MN_DEF_NODE_ID, NodeId};
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

    /// Internal function to process a deserialized `PowerlinkFrame`.
    fn process_frame(&mut self, frame: PowerlinkFrame, current_time_us: u64) -> NodeAction {
        // --- Special handling for SDO frames ---
        // Check if it's an ASnd frame *targeted at us* and has the SDO Service ID
        if let PowerlinkFrame::ASnd(ref asnd_frame) = frame {
             if asnd_frame.destination == self.nmt_state_machine.node_id && asnd_frame.service_id == ServiceId::Sdo {
                debug!("Received SDO/ASnd frame for processing.");
                // Extract SDO Headers first to get Transaction ID for potential abort
                let transaction_id = if asnd_frame.payload.len() >= 8 {
                    // Seq header(4) + Command header(at least 4 more)
                    SdoCommand::deserialize(&asnd_frame.payload[4..])
                        .ok()
                        .map(|cmd| cmd.header.transaction_id)
                } else {
                    None
                };

                let response_frame = match self
                    .sdo_server
                    .handle_request(&asnd_frame.payload, &mut self.od)
                {
                    Ok(response_payload) => {
                        // Build and send the normal SDO response
                        let response_asnd = crate::frame::ASndFrame::new( // Use crate path
                            self.mac_address,
                            asnd_frame.eth_header.source_mac,
                            asnd_frame.source, // Respond to the original source node
                            self.nmt_state_machine.node_id,
                            ServiceId::Sdo,
                            response_payload,
                        );
                        info!("Sending SDO response.");
                        Some(PowerlinkFrame::ASnd(response_asnd))
                    }
                    Err(e) => {
                        error!("SDO server error: {:?}", e);
                        // --- Send SDO Abort Frame ---
                        if let Some(tid) = transaction_id {
                            // Map PowerlinkError to SDO Abort Code (spec App. 3.10)
                            let abort_code = match e {
                                PowerlinkError::ObjectNotFound => 0x0602_0000,
                                PowerlinkError::SubObjectNotFound => 0x0609_0011,
                                PowerlinkError::TypeMismatch => 0x0607_0010,
                                // Added more specific abort codes based on context
                                PowerlinkError::SdoInvalidCommandPayload => 0x0504_0001, // Command specifier invalid
                                PowerlinkError::StorageError("Object is read-only") => 0x0601_0002,
                                PowerlinkError::StorageError(_) => 0x0800_0020, // Cannot transfer data
                                PowerlinkError::ValidationError(_) => 0x0800_0022, // Because of device state (likely config issue)
                                PowerlinkError::SdoSequenceError(_) => 0x0504_0003, // Invalid sequence number
                                _ => 0x0800_0000, // General error
                            };

                            Some(payload::build_sdo_abort_response(
                                self.mac_address,
                                self.nmt_state_machine.node_id,
                                &self.sdo_server,
                                tid,
                                abort_code,
                                asnd_frame.source, // Abort goes back to original sender NodeId
                                asnd_frame.eth_header.source_mac, // Abort goes back to original sender MAC
                            ))
                        } else {
                            error!(
                                "Cannot send SDO Abort: Could not determine Transaction ID from invalid request."
                            );
                            None
                        }
                    }
                };
                // Use common serialization path at the end of the function
                if let Some(response) = response_frame {
                    return self.serialize_and_prepare_action(response);
                } else {
                    return NodeAction::NoAction; // Explicitly return NoAction if SDO handling results in None
                }
            } else if asnd_frame.destination == self.nmt_state_machine.node_id {
                 // It's an ASnd for us, but not SDO. Log it.
                 trace!("Received non-SDO ASnd frame: {:?}", asnd_frame);
             } else {
                 // ASnd not for us, ignore silently.
                 return NodeAction::NoAction;
             }
        }

        // --- Handle SoC Frame specific logic ---
        if let PowerlinkFrame::Soc(_) = &frame {
            trace!("SoC received at time {}", current_time_us);
            self.last_soc_reception_time_us = current_time_us;
            self.soc_timeout_check_active = true;
            // A CN's cycle is defined by the reception of an SoC.
            // This is the correct place to decrement threshold counters.
            self.dll_error_manager.on_cycle_complete();

            // Schedule the next SoC timeout check.
            // Read cycle time and tolerance only once
            let cycle_time_opt = self.od.read_u32(OD_IDX_CYCLE_TIME, 0).map(|v| v as u64);
            let tolerance_opt = self.od.read_u32(OD_IDX_LOSS_SOC_TOLERANCE, 0).map(|v| v as u64);

            if let (Some(cycle_time_us), Some(tolerance_ns)) = (cycle_time_opt, tolerance_opt) {
                if cycle_time_us > 0 {
                    let tolerance_us = tolerance_ns / 1000;
                    let deadline = current_time_us + cycle_time_us + tolerance_us;
                    // Update next_tick_us only if this deadline is earlier or no deadline exists
                     match self.next_tick_us {
                        Some(current_deadline) if deadline < current_deadline => {
                            self.next_tick_us = Some(deadline);
                            trace!("Scheduled SoC timeout check at {}us (earlier)", deadline);
                        }
                        None => {
                            self.next_tick_us = Some(deadline);
                             trace!("Scheduled SoC timeout check at {}us (first)", deadline);
                        }
                         _ => { /* Existing deadline is earlier or equal, keep it */ }
                    }
                } else {
                     warn!("Cycle Time (0x1006) is 0, cannot schedule SoC timeout.");
                     self.soc_timeout_check_active = false; // Disable check if cycle time is zero
                }
            } else {
                warn!("Could not read Cycle Time (0x1006) or SoC Tolerance (0x1C14) from OD. SoC timeout check disabled.");
                self.soc_timeout_check_active = false; // Disable check if OD read fails
            }
        }

        // --- Handle EA/ER flags for error signaling handshake ---
        // Ensure frame is targeted at this node before processing PReq/SoA flags
        let target_node_id = match &frame {
            PowerlinkFrame::PReq(preq) => Some(preq.destination),
            PowerlinkFrame::SoA(soa) => Some(soa.target_node_id),
            _ => None,
        };

         if target_node_id == Some(self.nmt_state_machine.node_id) || target_node_id == Some(NodeId(crate::types::C_ADR_BROADCAST_NODE_ID)) {
             match &frame {
                PowerlinkFrame::PReq(preq) => {
                    // If MN acknowledges our error (EA matches EN), we can potentially change status data again.
                    // Spec 6.5.6: If EN == EA, CN may change StatusResponse data again.
                    if preq.flags.ea == self.en_flag {
                         trace!("Received matching EA flag ({}) from MN in PReq.", preq.flags.ea);
                        // The application logic might use this information, but the core state machine doesn't need to react here.
                        // The logic in `process_frame` handles toggling EN based on `error_status_changed`.
                    } else {
                        trace!("Received mismatched EA flag ({}, EN is {}) from MN in PReq.", preq.flags.ea, self.en_flag);
                    }
                }
                PowerlinkFrame::SoA(soa) => {
                    if soa.target_node_id == self.nmt_state_machine.node_id { // Only process SoA addressed to us
                        if soa.flags.er {
                            // MN requests reset of error signaling via ER flag.
                            info!("Received ER flag from MN in SoA, resetting EN flag and Emergency Queue.");
                            self.en_flag = false;
                            self.emergency_queue.clear();
                        }
                        self.ec_flag = soa.flags.er; // EC mirrors the received ER flag
                        trace!("Processed SoA flags: ER={}, EC set to {}", soa.flags.er, self.ec_flag);
                    }
                }
                _ => {} // Other frame types don't carry EA/ER flags for CNs
            }
        }


        // --- Normal Frame Processing ---

        // 1. Update NMT state machine based on the frame type or internal events.
         // Pass relevant NMT events triggered by frames.
         let nmt_event = match &frame {
            PowerlinkFrame::Soc(_) => Some(NmtEvent::SocReceived),
            PowerlinkFrame::SoA(_) => Some(NmtEvent::SocSoAReceived), // SoA also implies EPL mode entered
             // Explicit NMT commands via ASnd (handle separately if needed)
             PowerlinkFrame::ASnd(asnd) if asnd.destination == self.nmt_state_machine.node_id && asnd.service_id == ServiceId::NmtCommand => {
                if let Some(cmd_byte) = asnd.payload.get(0) {
                     match NmtCommand::try_from(*cmd_byte) {
                         Ok(NmtCommand::StartNode) => Some(NmtEvent::StartNode),
                         Ok(NmtCommand::StopNode) => Some(NmtEvent::StopNode),
                         Ok(NmtCommand::EnterPreOperational2) => Some(NmtEvent::EnterPreOperational2),
                         Ok(NmtCommand::EnableReadyToOperate) => Some(NmtEvent::EnableReadyToOperate),
                         Ok(NmtCommand::ResetNode) => Some(NmtEvent::ResetNode),
                         Ok(NmtCommand::ResetCommunication) => Some(NmtEvent::ResetCommunication),
                         Ok(NmtCommand::ResetConfiguration) => Some(NmtEvent::ResetConfiguration),
                         Ok(NmtCommand::SwReset) => Some(NmtEvent::SwReset),
                         Err(_) => {
                             warn!("Received ASnd with unknown NMT Command ID: {:#04x}", cmd_byte);
                             None
                         }
                     }
                 } else {
                     warn!("Received ASnd NMT Command with empty payload.");
                     None
                 }
             }
            _ => None,
        };

        if let Some(event) = nmt_event {
             // Pass the event to the NMT state machine
            self.nmt_state_machine.process_event(event, &mut self.od);
             // Note: NMT resets handled within process_event might trigger run_internal_initialisation
        }

        // 2. Update DLL state machine based on the frame type.
        // Get the DLL event corresponding to the received frame.
         let dll_event = frame.dll_cn_event();
         if let Some(errors) = self
            .dll_state_machine
            .process_event(dll_event, self.nmt_state_machine.current_state()) // Pass current NMT state
        {
            // If the DLL state machine detects an error (e.g., sequence error), handle it.
            for error in errors {
                warn!("DLL state machine reported error: {:?}", error);
                // Pass the error to the DLL error manager.
                 // Capture NMT action triggered by DLL error
                let (nmt_action, signaled) = self.dll_error_manager.handle_error(error);
                if signaled {
                     // Set flag to toggle EN bit before next PRes/StatusResponse
                    self.error_status_changed = true;
                    // --- Update Error Register (0x1001) ---
                    let current_err_reg = self.od.read_u8(OD_IDX_ERROR_REGISTER, 0).unwrap_or(0);
                    // Set Bit 0: Generic Error
                    let new_err_reg = current_err_reg | 0b1;
                    self.od.write_internal(
                        OD_IDX_ERROR_REGISTER,
                        0,
                        crate::od::ObjectValue::Unsigned8(new_err_reg),
                        false
                    ).unwrap_or_else(|e| error!("[CN] Failed to update Error Register: {:?}", e));

                    // Create and queue a detailed error entry.
                    let error_entry = ErrorEntry {
                        entry_type: EntryType {
                            is_status_entry: false,
                            send_to_queue: true,
                            mode: ErrorEntryMode::EventOccurred,
                            profile: 0x002, // POWERLINK communication profile
                        },
                        error_code: error.to_error_code(),
                        timestamp: NetTime {
                            seconds: (current_time_us / 1_000_000) as u32,
                            nanoseconds: ((current_time_us % 1_000_000) * 1000) as u32,
                        },
                        // Additional information could be context-specific (e.g., NodeId for LossOfPres)
                        additional_information: 0,
                    };
                    self.emergency_queue.push_back(error_entry);
                    info!("[CN] New error queued: {:?}", error_entry);
                }
                 // If the DLL error requires an NMT action (like ResetCommunication), trigger NMT error event.
                 if nmt_action != NmtAction::None {
                     // Spec Table 27 maps most threshold errors to NMT_CT11 (Error Condition -> PreOp1)
                     info!("DLL error triggered NMT action: {:?}", nmt_action);
                    self.nmt_state_machine.process_event(NmtEvent::Error, &mut self.od);
                     // If NMT state changed significantly (e.g., reset), we might skip response generation below.
                 }
            }
        }


        // 3. Handle PDO consumption *before* generating a response
        // Only consume if targeted at this node or broadcast (PRes)
        let is_target_or_broadcast = match &frame {
             PowerlinkFrame::PReq(f) => f.destination == self.nmt_state_machine.node_id,
             PowerlinkFrame::PRes(_) => true, // PRes is multicast
             _ => false,
        };

         if is_target_or_broadcast {
            match &frame {
                PowerlinkFrame::PReq(preq_frame) => {
                     // Only consume PReq if it's for us
                     if preq_frame.destination == self.nmt_state_machine.node_id {
                         self.consume_preq_payload(preq_frame);
                     }
                 }
                PowerlinkFrame::PRes(pres_frame) => self.consume_pres_payload(pres_frame),
                _ => {} // Other frames do not carry consumer PDOs
            }
        }


        // Check if we need to toggle the EN flag before building a response.
        // This should happen *after* processing errors but *before* building PRes/StatusResponse.
        if self.error_status_changed {
            self.en_flag = !self.en_flag;
            self.error_status_changed = false; // Reset the trigger immediately after toggling
            info!("New error detected or acknowledged, toggling EN flag to: {}", self.en_flag);
        }

        // 4. Generate response frames (only if the NMT state didn't just reset).
        // Check current NMT state *after* potential updates from errors.
         let current_nmt_state = self.nmt_state();
         let response_frame = if current_nmt_state >= NmtState::NmtNotActive { // Avoid response if Off/Initialising
             match &frame {
                PowerlinkFrame::SoA(soa_frame) => {
                    // Only respond to SoA specifically addressed to this node
                    if soa_frame.target_node_id == self.nmt_state_machine.node_id {
                        match current_nmt_state {
                            // Per Table 108, ASnd responses allowed in PreOp1 and PreOp2.
                            NmtState::NmtPreOperational1 | NmtState::NmtPreOperational2 => {
                                match soa_frame.req_service_id {
                                    RequestedServiceId::IdentRequest => {
                                        Some(payload::build_ident_response(
                                            self.mac_address,
                                            self.nmt_state_machine.node_id,
                                            &self.od,
                                            soa_frame,
                                        ))
                                    }
                                    RequestedServiceId::StatusRequest => {
                                        Some(payload::build_status_response(
                                            self.mac_address,
                                            self.nmt_state_machine.node_id,
                                            &mut self.od,
                                            self.en_flag, // Pass current EN flag
                                            self.ec_flag, // Pass current EC flag
                                            &mut self.emergency_queue,
                                            soa_frame,
                                        ))
                                    }
                                    RequestedServiceId::NmtRequestInvite => {
                                         // Dequeue and build NMTRequest if available
                                        if let Some((command, target)) = self.pending_nmt_requests.pop()
                                        {
                                            info!("Responding to NmtRequestInvite with queued request: {:?}, target {}", command, target.0);
                                            Some(payload::build_nmt_request(
                                                self.mac_address,
                                                self.nmt_state_machine.node_id,
                                                command,
                                                target,
                                                soa_frame,
                                            ))
                                        } else {
                                            warn!(
                                                "Received NmtRequestInvite but have no pending NMT requests."
                                            );
                                            None // No pending request, send nothing
                                        }
                                    }
                                    RequestedServiceId::UnspecifiedInvite => {
                                         // Dequeue and build SDO Request if available
                                        if let Some(sdo_payload) = self.sdo_server.pop_pending_request()
                                        {
                                            info!(
                                                "Responding to UnspecifiedInvite with queued SDO request ({} bytes).", sdo_payload.len()
                                            );
                                            let asnd = crate::frame::ASndFrame::new( // Use crate path
                                                self.mac_address,
                                                soa_frame.eth_header.source_mac, // Target MN MAC
                                                NodeId(C_ADR_MN_DEF_NODE_ID), // Target MN Node ID
                                                self.nmt_state_machine.node_id, // Source Node ID
                                                ServiceId::Sdo,
                                                sdo_payload,
                                            );
                                            Some(PowerlinkFrame::ASnd(asnd))
                                        } else {
                                             trace!( // Use trace as this is normal if queue is empty
                                                "Received UnspecifiedInvite but have no pending SDO requests."
                                            );
                                            None // No pending SDO, send nothing
                                        }
                                    }
                                    RequestedServiceId::NoService => None, // No response needed for NoService invite
                                }
                            }
                            _ => None, // No ASnd responses expected in other states
                        }
                    } else {
                        None // SoA not targeted at us
                    }
                }
                PowerlinkFrame::PReq(preq_frame) => {
                     // Only respond to PReq specifically addressed to this node
                     if preq_frame.destination == self.nmt_state_machine.node_id {
                        match current_nmt_state {
                            // Per Table 108, PRes response allowed in PreOp2, ReadyToOp, Op.
                            NmtState::NmtPreOperational2
                            | NmtState::NmtReadyToOperate
                            | NmtState::NmtOperational => Some(payload::build_pres_response(
                                self.mac_address,
                                self.nmt_state_machine.node_id,
                                current_nmt_state, // Pass the current NMT state
                                &self.od,
                                &self.sdo_server,
                                &self.pending_nmt_requests, // Pass pending NMT requests for RS/PR flags
                                self.en_flag, // Pass current EN flag
                            )),
                            _ => None, // No PRes response in other states
                        }
                    } else {
                        None // PReq not targeted at us
                    }
                }
                _ => None, // No response needed for received SoC, PRes, or non-SDO ASnd
            }
        } else {
            None // NMT state is Off or Initialising, don't generate responses
        };


        if let Some(response) = response_frame {
            return self.serialize_and_prepare_action(response);
        }

        NodeAction::NoAction
    }


    /// Helper to serialize a PowerlinkFrame and prepare the NodeAction.
    fn serialize_and_prepare_action(&self, frame: PowerlinkFrame) -> NodeAction {
        // Estimate max size needed (14 Eth + Max PL size ~1500)
        let mut buf = vec![0u8; 1518];
        // Serialize Eth header first
        let eth_header = match &frame {
            PowerlinkFrame::PRes(f) => f.eth_header,
            PowerlinkFrame::ASnd(f) => f.eth_header,
            // Add other frame types if CN might send them (unlikely for responses)
            _ => {
                error!("[CN] Attempted to serialize unexpected response frame type: {:?}", frame);
                return NodeAction::NoAction;
            }
        };
        CodecHelpers::serialize_eth_header(&eth_header, &mut buf);

        // Then serialize PL part into the buffer starting after the Eth header
        match frame.serialize(&mut buf[14..]) {
            Ok(pl_size) => {
                let total_size = 14 + pl_size;
                 if total_size < 60 { // Ethernet minimum frame size (excluding preamble, SFD, FCS)
                    buf.resize(60, 0); // Pad with zeros if needed
                    trace!("Padding frame from {} to 60 bytes.", total_size);
                 } else {
                    buf.truncate(total_size);
                 }
                info!("Sending response frame type: {:?}", frame); // Log frame type for clarity
                trace!("Sending frame bytes ({}): {:02X?}", buf.len(), &buf);
                NodeAction::SendFrame(buf)
            }
            Err(e) => {
                error!("[CN] Failed to serialize response frame: {:?}", e);
                NodeAction::NoAction
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
    fn process_raw_frame(&mut self, buffer: &[u8], current_time_us: u64) -> NodeAction {
        // First, check if we are in BasicEthernet and if this is a POWERLINK frame
        if self.nmt_state() == NmtState::NmtBasicEthernet
            && buffer.len() >= 14 // Check length before slicing
            && buffer[12..14] == crate::types::C_DLL_ETHERTYPE_EPL.to_be_bytes()
        {
            info!(
                "[CN] POWERLINK frame detected in NmtBasicEthernet. Transitioning to NmtPreOperational1."
            );
             // Trigger the NMT transition
            self.nmt_state_machine
                .process_event(NmtEvent::PowerlinkFrameReceived, &mut self.od);
             // After transitioning, immediately process the frame that triggered it
             // Fall through to the deserialize_frame logic below.
        }

        match deserialize_frame(buffer) {
            Ok(frame) => self.process_frame(frame, current_time_us),
            Err(PowerlinkError::InvalidEthernetFrame) => {
                // Not a POWERLINK frame (wrong EtherType), ignore silently.
                trace!("Ignoring non-POWERLINK frame (wrong EtherType).");
                NodeAction::NoAction
            }
            Err(e) => {
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
                }
                 // Trigger NMT error handling if required
                if nmt_action != NmtAction::None {
                    self.nmt_state_machine.process_event(NmtEvent::Error, &mut self.od);
                }
                NodeAction::NoAction
            }
        }
    }

    fn tick(&mut self, current_time_us: u64) -> NodeAction {
        let current_nmt_state = self.nmt_state();
         // Check if a deadline is set and if it has passed
        let deadline_passed = self.next_tick_us.map_or(false, |deadline| current_time_us >= deadline);

        // Special case for NmtNotActive: the first time tick is called, start the timer if needed.
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

        // If no deadline has passed, do nothing.
        if !deadline_passed {
            return NodeAction::NoAction;
        }

        // --- A deadline has passed ---
        trace!("Tick deadline reached at {}us (Deadline was {:?})", current_time_us, self.next_tick_us);
        self.next_tick_us = None; // Consume the deadline that just passed

        // Check for NmtNotActive timeout -> BasicEthernet
        if current_nmt_state == NmtState::NmtNotActive {
             // BasicEthernet timeout is handled here based on the deadline check above.
             let timeout_us = self.nmt_state_machine.basic_ethernet_timeout as u64;
             if timeout_us > 0 { // Only trigger if timeout was actually enabled
                warn!("BasicEthernet timeout expired. Transitioning state.");
                self.nmt_state_machine.process_event(NmtEvent::Timeout, &mut self.od);
                self.soc_timeout_check_active = false; // Ensure SoC check is off in BasicEthernet
             }
             // No further action needed this tick after state change
             return NodeAction::NoAction;
        }
        // Check for SoC timeout
        else if self.soc_timeout_check_active {
            warn!("SoC timeout detected at {}us! Last SoC was at {}us.", current_time_us, self.last_soc_reception_time_us);
             // Report SoC timeout event to DLL state machine
            if let Some(errors) = self
                .dll_state_machine
                .process_event(DllCsEvent::SocTimeout, current_nmt_state) // Pass current NMT state
            {
                 // Process any DLL errors resulting from the timeout event
                for error in errors {
                    let (nmt_action, signaled) = self.dll_error_manager.handle_error(error);
                    if signaled {
                        self.error_status_changed = true;
                    }
                     // If DLL error triggers NMT action (like reset), handle it
                    if nmt_action != NmtAction::None {
                        self.nmt_state_machine.process_event(NmtEvent::Error, &mut self.od);
                        self.soc_timeout_check_active = false; // Stop checking SoC after NMT error/reset
                         // After NMT reset, no further action this tick
                         return NodeAction::NoAction;
                    }
                }
            }

             // If still active (no NMT reset occurred), schedule the next timeout check
             // based on the last *expected* SoC time.
             if self.soc_timeout_check_active {
                 let cycle_time_opt = self.od.read_u32(OD_IDX_CYCLE_TIME, 0).map(|v| v as u64);
                 let tolerance_opt = self.od.read_u32(OD_IDX_LOSS_SOC_TOLERANCE, 0).map(|v| v as u64);

                if let (Some(cycle_time_us), Some(tolerance_ns)) = (cycle_time_opt, tolerance_opt) {
                     if cycle_time_us > 0 {
                         // Assume SoC should have arrived around the deadline we just met.
                         // Calculate next deadline based on when the *last* SoC *was* received + multiples of cycle time.
                         // Find the next cycle boundary *after* the current time.
                         let cycles_missed = ((current_time_us - self.last_soc_reception_time_us) / cycle_time_us) + 1;
                         let next_expected_soc_time = self.last_soc_reception_time_us + cycles_missed * cycle_time_us;
                         let next_deadline = next_expected_soc_time + (tolerance_ns / 1000);
                        self.next_tick_us = Some(next_deadline);
                         trace!("SoC timeout occurred, scheduling next check at {}us", next_deadline);
                     } else {
                         self.soc_timeout_check_active = false; // Disable check if cycle time became zero
                     }
                 } else {
                     self.soc_timeout_check_active = false; // Disable check if OD read fails
                 }
            }
        } else {
             trace!("Tick deadline reached, but no specific timeout active (State: {:?}).", current_nmt_state);
             // Potentially check other application timers here if needed.
             // If nothing else to do, we might not reschedule next_tick_us until the next SoC arrives.
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
    use crate::{nmt::flags::FeatureFlags, od::{AccessType, Category, Object, ObjectEntry, ObjectValue, PdoMapping}};
    use super::*;

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
                default_value: None, value_range: None, pdo_mapping: None,
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
                access: None, default_value: None, value_range: None, pdo_mapping: None,
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
                default_value: None, value_range: None, pdo_mapping: None,
            },
        );

         // --- CN Specific Mandatory Objects ---
        od.insert(
            0x1F93, // NMT_EPLNodeID_REC
            ObjectEntry {
                object: Object::Record(vec![ObjectValue::Unsigned8(node_id), ObjectValue::Boolean(0)]),
                name: "NodeIDConfig",
                category: Category::Mandatory,
                access: None, default_value: None, value_range: None, pdo_mapping: None,
            },
        );
         od.insert(
            0x1F99, // NMT_CNBasicEthernetTimeout_U32
            ObjectEntry {
                object: Object::Variable(ObjectValue::Unsigned32(5_000_000)), // 5 seconds
                name: "BasicEthTimeout",
                category: Category::Mandatory,
                access: Some(AccessType::ReadWriteStore),
                default_value: None, value_range: None, pdo_mapping: None,
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
                default_value: None, value_range: None, pdo_mapping: Some(PdoMapping::No), // Spec lists mapping as No
            },
        );
         od.insert(
            0x1006, // NMT_CycleLen_U32 (Needed for SoC timeout scheduling)
            ObjectEntry {
                object: Object::Variable(ObjectValue::Unsigned32(10000)), // 10ms example
                name: "CycleLength",
                category: Category::Mandatory, // Spec lists as Mandatory
                access: Some(AccessType::ReadWriteStore),
                default_value: None, value_range: None, pdo_mapping: None,
            },
        );
         od.insert(
            0x1C14, // DLL_CNLossOfSocTolerance_U32 (Needed for SoC timeout scheduling)
            ObjectEntry {
                object: Object::Variable(ObjectValue::Unsigned32(100000)), // 100us example
                name: "LossSocTolerance",
                category: Category::Mandatory, // Spec lists as Mandatory for CN
                access: Some(AccessType::ReadWriteStore),
                default_value: None, value_range: None, pdo_mapping: None,
            },
        );
         // Add minimal PDO config objects required by payload::build_pres_response
         od.insert(
            0x1800, // TPDO Comm Param (for PRes)
             ObjectEntry {
                object: Object::Record(vec![ObjectValue::Unsigned8(node_id), ObjectValue::Unsigned8(0)]),
                name: "TPDO1CommParam", category: Category::Mandatory, access: None,
                 default_value: None, value_range: None, pdo_mapping: None,
            }
         );
          od.insert(
            0x1A00, // TPDO Mapping Param (for PRes)
              ObjectEntry {
                object: Object::Array(vec![]), // Empty mapping
                name: "TPDO1MapParam", category: Category::Mandatory, access: None,
                 default_value: None, value_range: None, pdo_mapping: None,
            }
         );
         od.insert(
            0x1F98, // NMT_CycleTiming_REC (needed for PresActPayloadLimit)
             ObjectEntry {
                object: Object::Record(vec![
                    ObjectValue::Unsigned16(1490), ObjectValue::Unsigned16(1490),
                    ObjectValue::Unsigned32(10000), ObjectValue::Unsigned16(100),
                    ObjectValue::Unsigned16(36), // PresActPayloadLimit_U16 = 36
                    ObjectValue::Unsigned32(20000), ObjectValue::Unsigned8(0),
                    ObjectValue::Unsigned16(300), ObjectValue::Unsigned16(2),
                ]),
                name: "CycleTiming", category: Category::Mandatory, access: None,
                 default_value: None, value_range: None, pdo_mapping: None,
            }
         );
         od.insert(
            0x1001, // ERR_ErrorRegister_U8 (used in build_status_response)
             ObjectEntry {
                object: Object::Variable(ObjectValue::Unsigned8(0)),
                name: "ErrorRegister", category: Category::Mandatory,
                access: Some(AccessType::ReadOnly), default_value: None,
                value_range: None, pdo_mapping: Some(PdoMapping::Optional),
            }
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
         assert!(matches!(result, Err(PowerlinkError::ValidationError("Missing common mandatory object"))));
    }

    #[test]
    fn test_internal_boot_sequence() {
        let mut od = get_test_od(); // Use the corrected OD
        // Create the node, which runs init() and validate() internally
        let mut node = ControlledNode::new(od, MacAddress([0; 6])).unwrap();
        // The constructor already runs run_internal_initialisation.
        assert_eq!(node.nmt_state(), NmtState::NmtNotActive);
        // Verify OD state was updated
         assert_eq!(node.od.read_u8(0x1F8C, 0), Some(NmtState::NmtNotActive as u8));
    }


    #[test]
    fn test_full_boot_up_happy_path() {
        let mut od = get_test_od();
        // Create node, runs init, validate, internal_init -> NmtNotActive
        let mut node = ControlledNode::new(od, MacAddress([0; 6])).unwrap();
        assert_eq!(node.nmt_state(), NmtState::NmtNotActive);

        // NMT_CT2: Receive SoA or SoC
        node.nmt_state_machine.process_event(NmtEvent::SocSoAReceived, &mut node.od);
        assert_eq!(node.nmt_state(), NmtState::NmtPreOperational1);

        // NMT_CT4: Receive SoC
        node.nmt_state_machine.process_event(NmtEvent::SocReceived, &mut node.od);
        assert_eq!(node.nmt_state(), NmtState::NmtPreOperational2);

        // NMT_CT5: Receive EnableReadyToOperate (state doesn't change yet)
        node.nmt_state_machine.process_event(NmtEvent::EnableReadyToOperate, &mut node.od);
        assert_eq!(node.nmt_state(), NmtState::NmtPreOperational2);

        // NMT_CT6: Application signals completion
        node.nmt_state_machine.process_event(NmtEvent::CnConfigurationComplete, &mut node.od);
        assert_eq!(node.nmt_state(), NmtState::NmtReadyToOperate);

        // NMT_CT7: Receive StartNode
        node.nmt_state_machine.process_event(NmtEvent::StartNode, &mut node.od);
        assert_eq!(node.nmt_state(), NmtState::NmtOperational);
        assert_eq!(node.od.read_u8(0x1F8C, 0), Some(NmtState::NmtOperational as u8));
    }

    #[test]
    fn test_error_handling_transition() {
        let mut od = get_test_od();
        let mut node = ControlledNode::new(od, MacAddress([0; 6])).unwrap();
        // Manually set state to Operational for test
        node.nmt_state_machine.current_state = NmtState::NmtOperational;
        node.nmt_state_machine.update_od_state(&mut node.od);


        // NMT_CT11: Trigger internal error
        node.nmt_state_machine.process_event(NmtEvent::Error, &mut node.od);
        assert_eq!(node.nmt_state(), NmtState::NmtPreOperational1);
         assert_eq!(node.od.read_u8(0x1F8C, 0), Some(NmtState::NmtPreOperational1 as u8));
    }

    #[test]
    fn test_stop_and_restart_node() {
        let mut od = get_test_od();
        let mut node = ControlledNode::new(od, MacAddress([0; 6])).unwrap();
        // Manually set state to Operational
        node.nmt_state_machine.current_state = NmtState::NmtOperational;
         node.nmt_state_machine.update_od_state(&mut node.od);

        // NMT_CT8: Receive StopNode
        node.nmt_state_machine.process_event(NmtEvent::StopNode, &mut node.od);
        assert_eq!(node.nmt_state(), NmtState::NmtCsStopped);
        assert_eq!(node.od.read_u8(0x1F8C, 0), Some(NmtState::NmtCsStopped as u8));


        // NMT_CT10: Receive EnterPreOperational2
        node.nmt_state_machine.process_event(NmtEvent::EnterPreOperational2, &mut node.od);
        assert_eq!(node.nmt_state(), NmtState::NmtPreOperational2);
         assert_eq!(node.od.read_u8(0x1F8C, 0), Some(NmtState::NmtPreOperational2 as u8));
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