use super::payload;
use crate::PowerlinkError;
use crate::frame::{
    ASndFrame, Codec, DllCsEvent, DllCsStateMachine, DllError, NmtAction, PReqFrame, PResFrame,
    PowerlinkFrame, RequestedServiceId, ServiceId, SoAFrame,
    basic::MacAddress,
    // deserialize_frame is now only used in process_raw_frame
    deserialize_frame,
    error::{CnErrorCounters, DllErrorManager, ErrorCounters, ErrorHandler, LoggingErrorHandler},
};
// Import the trait for SDO payload (de)serialization
use crate::frame::codec::CodecHelpers;
use crate::nmt::cn_state_machine::CnNmtStateMachine;
use crate::nmt::events::{NmtCommand, NmtEvent};
use crate::nmt::state_machine::NmtStateMachine;
use crate::nmt::states::NmtState;
use crate::node::{Node, NodeAction, PdoHandler};
use crate::od::ObjectDictionary;
use crate::sdo::SdoServer;
// SdoCommand and Headers are needed for SDO logic
use crate::sdo::command::SdoCommand;
use crate::sdo::sequence::SequenceLayerHeader;
use crate::types::{C_ADR_MN_DEF_NODE_ID, NodeId};
use alloc::vec;
use alloc::vec::Vec;
use log::{debug, error, info, trace, warn};

// Constants for OD access
const OD_IDX_CYCLE_TIME: u16 = 0x1006;
const OD_IDX_LOSS_SOC_TOLERANCE: u16 = 0x1C14;

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
            last_soc_reception_time_us: 0,
            soc_timeout_check_active: false,
            next_tick_us: None,
            en_flag: false,
            // Per spec 6.5.5.1, EC starts as 1 to indicate "not initialized"
            ec_flag: true,
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

    /// Internal function to process a deserialized `PowerlinkFrame`.
    fn process_frame(&mut self, frame: PowerlinkFrame, current_time_us: u64) -> NodeAction {
        // --- Special handling for SDO frames ---
        if let PowerlinkFrame::ASnd(ref asnd_frame) = frame {
            if asnd_frame.service_id == ServiceId::Sdo {
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
                        let response_asnd = ASndFrame::new(
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
                                PowerlinkError::SdoInvalidCommandPayload => 0x0800_0000, // General error
                                PowerlinkError::StorageError("Object is read-only") => 0x0601_0002,
                                PowerlinkError::StorageError(_) => 0x0800_0020, // Cannot transfer data
                                _ => 0x0800_0000,                               // General error
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
                }
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
            if let (Some(cycle_time_us), Some(tolerance_ns)) = (
                self.od.read_u32(OD_IDX_CYCLE_TIME, 0).map(|v| v as u64),
                self.od
                    .read_u32(OD_IDX_LOSS_SOC_TOLERANCE, 0)
                    .map(|v| v as u64),
            ) {
                if cycle_time_us > 0 {
                    let tolerance_us = tolerance_ns / 1000;
                    let deadline = current_time_us + cycle_time_us + tolerance_us;
                    self.next_tick_us = Some(deadline);
                }
            }
        }

        // --- Handle EA/ER flags for error signaling handshake ---
        match &frame {
            PowerlinkFrame::PReq(preq) => {
                if preq.flags.ea == self.en_flag {
                    // TODO: Acknowledged, CN may change StatusResponse data again.
                }
            }
            PowerlinkFrame::SoA(soa) => {
                if soa.flags.er {
                    // MN requests reset of error signaling.
                    self.en_flag = false;
                    // TODO: Clear emergency queue.
                }
                self.ec_flag = soa.flags.er; // EC mirrors ER
            }
            _ => {}
        }

        // --- Normal Frame Processing ---

        // 1. Update NMT state machine based on the frame type.
        if let Some(event) = frame.nmt_event() {
            self.nmt_state_machine.process_event(event, &mut self.od);
        }

        // 2. Update DLL state machine based on the frame type.
        if let Some(errors) = self
            .dll_state_machine
            .process_event(frame.dll_cn_event(), self.nmt_state_machine.current_state())
        {
            // If the DLL detects an error (like a lost frame), pass it to the error manager.
            for error in errors {
                warn!("DLL state machine reported error: {:?}", error);
                if self.dll_error_manager.handle_error(error) != NmtAction::None {
                    self.nmt_state_machine
                        .process_event(NmtEvent::Error, &mut self.od);
                }
            }
        }

        // 3. Handle PDO consumption *before* generating a response
        match &frame {
            PowerlinkFrame::PReq(preq_frame) => self.consume_preq_payload(preq_frame),
            PowerlinkFrame::PRes(pres_frame) => self.consume_pres_payload(pres_frame),
            _ => {} // Other frames do not carry consumer PDOs
        }

        // 4. Generate response frames (logic moved from FrameHandler trait).
        let response_frame = match &frame {
            PowerlinkFrame::SoA(soa_frame) => {
                if soa_frame.target_node_id != self.nmt_state_machine.node_id {
                    None // Not for us
                } else {
                    match self.nmt_state() {
                        // Per Table 108, these can be handled in PreOp1 and PreOp2.
                        NmtState::NmtPreOperational1 | NmtState::NmtPreOperational2 => {
                            match soa_frame.req_service_id {
                                RequestedServiceId::IdentRequest => Some(
                                    payload::build_ident_response(
                                        self.mac_address,
                                        self.nmt_state_machine.node_id,
                                        &self.od,
                                        soa_frame,
                                    ),
                                ),
                                RequestedServiceId::StatusRequest => Some(
                                    payload::build_status_response(
                                        self.mac_address,
                                        self.nmt_state_machine.node_id,
                                        &self.od,
                                        self.en_flag,
                                        self.ec_flag,
                                        soa_frame,
                                    ),
                                ),
                                RequestedServiceId::NmtRequestInvite => {
                                    if let Some((command, target)) = self.pending_nmt_requests.pop()
                                    {
                                        Some(payload::build_nmt_request(
                                            self.mac_address,
                                            self.nmt_state_machine.node_id,
                                            command,
                                            target,
                                            soa_frame,
                                        ))
                                    } else {
                                        warn!("Received NmtRequestInvite but have no pending requests.");
                                        None
                                    }
                                }
                                RequestedServiceId::UnspecifiedInvite => {
                                    if let Some(sdo_payload) = self.sdo_server.pop_pending_request() {
                                        info!("Received UnspecifiedInvite, sending queued SDO request.");
                                        let asnd = ASndFrame::new(
                                            self.mac_address,
                                            soa_frame.eth_header.source_mac,
                                            NodeId(C_ADR_MN_DEF_NODE_ID),
                                            self.nmt_state_machine.node_id,
                                            ServiceId::Sdo,
                                            sdo_payload,
                                        );
                                        Some(PowerlinkFrame::ASnd(asnd))
                                    } else {
                                        warn!("Received UnspecifiedInvite but have no pending SDO requests.");
                                        None
                                    }
                                }
                                _ => None,
                            }
                        }
                        _ => None,
                    }
                }
            }
            PowerlinkFrame::PReq(_) => {
                match self.nmt_state() {
                    // A CN only responds to PReq when in isochronous states.
                    NmtState::NmtPreOperational2
                    | NmtState::NmtReadyToOperate
                    | NmtState::NmtOperational => Some(payload::build_pres_response(
                        self.mac_address,
                        self.nmt_state_machine.node_id,
                        self.nmt_state(),
                        &self.od,
                        &self.sdo_server,
                        &self.pending_nmt_requests,
                    )),
                    _ => None,
                }
            }
            _ => None,
        };

        if let Some(response) = response_frame {
            return self.serialize_and_prepare_action(response);
        }

        NodeAction::NoAction
    }

    /// Helper to serialize a PowerlinkFrame and prepare the NodeAction.
    fn serialize_and_prepare_action(&self, frame: PowerlinkFrame) -> NodeAction {
        let mut buf = vec![0u8; 1500];
        // Serialize Eth header first
        let eth_header = match &frame {
            PowerlinkFrame::PRes(f) => f.eth_header,
            PowerlinkFrame::ASnd(f) => f.eth_header,
            _ => {
                // Should only be PRes or ASnd for CN responses.
                error!("Generated unexpected response frame type: {:?}", frame);
                return NodeAction::NoAction;
            }
        };
        CodecHelpers::serialize_eth_header(&eth_header, &mut buf);
        // Then serialize PL part
        match frame.serialize(&mut buf[14..]) {
            Ok(pl_size) => {
                let total_size = 14 + pl_size;
                buf.truncate(total_size);
                info!("Sending response frame: {:?}", frame);
                trace!("Sending frame bytes ({}): {:?}", total_size, &buf);
                NodeAction::SendFrame(buf)
            }
            Err(e) => {
                error!("Failed to serialize response frame: {:?}", e);
                NodeAction::NoAction
            }
        }
    }

    /// Consumes the payload of a PReq frame based on RPDO mapping.
    fn consume_preq_payload(&mut self, preq: &PReqFrame) {
        self.consume_pdo_payload(
            NodeId(0), // Node ID 0 is reserved for PReq
            &preq.payload,
            preq.pdo_version,
            preq.flags.rd,
        );
    }

    /// Consumes the payload of a PRes frame based on RPDO mapping.
    fn consume_pres_payload(&mut self, pres: &PResFrame) {
        self.consume_pdo_payload(
            pres.source, // Source Node ID of the PRes
            &pres.payload,
            pres.pdo_version,
            pres.flags.rd,
        );
    }
}

// Implement the PdoHandler trait for ControlledNode
impl<'s> PdoHandler<'s> for ControlledNode<'s> {
    fn od(&mut self) -> &mut ObjectDictionary<'s> {
        &mut self.od
    }

    // Match the trait signature using `impl Trait`
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
            self.nmt_state_machine
                .process_event(NmtEvent::PowerlinkFrameReceived, &mut self.od);
            // After transitioning, fall through to process the frame that triggered it
        }

        match deserialize_frame(buffer) {
            Ok(frame) => self.process_frame(frame, current_time_us),
            Err(PowerlinkError::InvalidEthernetFrame) => {
                // This is not a POWERLINK frame (e.g., ARP, IP), so we ignore it.
                // This is expected on a shared network interface.
                trace!("Ignoring non-POWERLINK frame (wrong EtherType).");
                NodeAction::NoAction
            }
            Err(e) => {
                // This looked like a POWERLINK frame (correct EtherType) but was malformed.
                // This is an error condition.
                warn!("[CN] Could not deserialize frame: {:?} (Buffer: {:?})", e, buffer);
                if self.dll_error_manager.handle_error(DllError::InvalidFormat) != NmtAction::None {
                    self.nmt_state_machine
                        .process_event(NmtEvent::Error, &mut self.od);
                }
                NodeAction::NoAction
            }
        }
    }

    fn tick(&mut self, current_time_us: u64) -> NodeAction {
        let current_nmt_state = self.nmt_state();
        let deadline = self.next_tick_us.unwrap_or(0);
        let deadline_passed = current_time_us >= deadline;

        // Special case for NmtNotActive: the first time tick is called, start the timer.
        if current_nmt_state == NmtState::NmtNotActive && self.next_tick_us.is_none() {
            let timeout_us = self.nmt_state_machine.basic_ethernet_timeout as u64;
            if timeout_us > 0 {
                debug!("No SoC seen, starting BasicEthernet timeout check.");
                self.next_tick_us = Some(current_time_us + timeout_us);
            }
            return NodeAction::NoAction; // Don't act on this first call, just set the timer.
        }

        if !deadline_passed {
            return NodeAction::NoAction;
        }

        // A deadline has passed, perform time-based actions.
        self.next_tick_us = None; // Consume the deadline

        if current_nmt_state == NmtState::NmtNotActive {
            warn!("BasicEthernet timeout expired. Transitioning state.");
            self.nmt_state_machine
                .process_event(NmtEvent::Timeout, &mut self.od);
            self.soc_timeout_check_active = false;
        } else if self.soc_timeout_check_active {
            warn!("SoC timeout detected at {}us!", current_time_us);
            if let Some(errors) = self
                .dll_state_machine
                .process_event(DllCsEvent::SocTimeout, current_nmt_state)
            {
                for error in errors {
                    if self.dll_error_manager.handle_error(error) != NmtAction::None {
                        self.nmt_state_machine
                            .process_event(NmtEvent::Error, &mut self.od);
                        self.soc_timeout_check_active = false;
                        return NodeAction::NoAction;
                    }
                }
            }
            // If still active, schedule the next timeout check based on the last *expected* SoC time.
            if let (Some(cycle_time_us), Some(tolerance_ns)) = (
                self.od.read_u32(OD_IDX_CYCLE_TIME, 0).map(|v| v as u64),
                self.od
                    .read_u32(OD_IDX_LOSS_SOC_TOLERANCE, 0)
                    .map(|v| v as u64),
            ) {
                if cycle_time_us > 0 {
                    // Assume SoC should have arrived at the deadline we just met
                    self.last_soc_reception_time_us += cycle_time_us;
                    let next_deadline =
                        self.last_soc_reception_time_us + cycle_time_us + (tolerance_ns / 1000);
                    self.next_tick_us = Some(next_deadline);
                }
            }
        }

        NodeAction::NoAction
    }

    fn nmt_state(&self) -> NmtState {
        self.nmt_state_machine.current_state()
    }

    fn next_action_time(&self) -> Option<u64> {
        self.next_tick_us
    }
}