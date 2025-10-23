// crates/powerlink-rs/src/node/cn/main.rs

use super::payload;
use crate::frame::{
    basic::MacAddress, deserialize_frame, 
    error::{CnErrorCounters, DllErrorManager, ErrorCounters, ErrorHandler, LoggingErrorHandler}, 
    ASndFrame, Codec, DllCsEvent, DllCsStateMachine, DllError, NmtAction, PReqFrame, PResFrame, 
    PowerlinkFrame, RequestedServiceId, ServiceId
};
use crate::nmt::cn_state_machine::CnNmtStateMachine;
use crate::nmt::state_machine::NmtStateMachine;
use crate::nmt::states::NmtState;
use crate::nmt::events::NmtEvent;
use crate::node::{Node, NodeAction, PdoHandler};
use crate::od::ObjectDictionary;
use crate::sdo::SdoServer;
use crate::types::NodeId;
use crate::PowerlinkError;
use alloc::vec;
use log::{debug, error, info, trace, warn};

// Constants for OD access
const OD_IDX_CYCLE_TIME: u16 = 0x1006;
const OD_IDX_LOSS_SOC_TOLERANCE: u16 = 0x1C14;

/// Represents a complete POWERLINK Controlled Node (CN).
/// This struct owns and manages all protocol layers and state machines.
pub struct ControlledNode<'s> {
    pub(super) od: ObjectDictionary<'s>,
    pub(super) nmt_state_machine: CnNmtStateMachine,
    dll_state_machine: DllCsStateMachine,
    dll_error_manager: DllErrorManager<CnErrorCounters, LoggingErrorHandler>,
    mac_address: MacAddress,
    sdo_server: SdoServer,
    /// Timestamp of the last successfully received SoC frame (microseconds).
    last_soc_reception_time_us: u64,
    /// Flag indicating if the SoC timeout check is currently active.
    soc_timeout_check_active: bool,
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
            last_soc_reception_time_us: 0,
            soc_timeout_check_active: false,
        };

        // Run the initial state transitions to get to NmtNotActive.
        node.nmt_state_machine
            .run_internal_initialisation(&mut node.od);

        Ok(node)
    }

    /// Internal function to process a deserialized `PowerlinkFrame`.
    fn process_frame(&mut self, frame: PowerlinkFrame, current_time_us: u64) -> NodeAction {
        // --- Special handling for SDO frames ---
        if let PowerlinkFrame::ASnd(ref asnd_frame) = frame {
            if asnd_frame.service_id == ServiceId::Sdo {
                debug!("Received SDO/ASnd frame for processing.");
                // Extract SDO Headers first to get Transaction ID for potential abort
                let transaction_id = if asnd_frame.payload.len() >= 4 { // Seq header
                    crate::sdo::sequence::SequenceLayerHeader::deserialize(&asnd_frame.payload[0..4])
                        .ok() // Ignore deserialization errors here, let SDO server handle payload
                        .and_then(|_seq_header| {
                             if asnd_frame.payload.len() >= 8 { // Cmd header
                                crate::sdo::command::SdoCommand::deserialize(&asnd_frame.payload[4..])
                                    .ok()
                                    .map(|cmd| cmd.header.transaction_id)
                             } else { None }
                        })
                } else {
                    None
                };

                match self
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
                        let mut buf = vec![0u8; 1500];
                        if let Ok(size) = response_asnd.serialize(&mut buf) {
                            buf.truncate(size);
                            info!("Sending SDO response.");
                            trace!("SDO response payload: {:?}", &buf);
                            return NodeAction::SendFrame(buf);
                        } else {
                            error!("Failed to serialize SDO response frame.");
                        }
                    }
                    Err(e) => {
                        error!("SDO server error: {:?}", e);
                        // --- Send SDO Abort Frame ---
                        if let Some(tid) = transaction_id {
                            // Map PowerlinkError to SDO Abort Code (simplified mapping)
                            let abort_code = match e {
                                PowerlinkError::ObjectNotFound => 0x0602_0000,
                                PowerlinkError::SubObjectNotFound => 0x0609_0011,
                                PowerlinkError::TypeMismatch => 0x0607_0010,
                                PowerlinkError::StorageError(_) => 0x0800_0020, // Cannot transfer data
                                _ => 0x0800_0000,                               // General error
                            };
                            return payload::build_sdo_abort_response(
                                self.mac_address,
                                self.nmt_state_machine.node_id,
                                &self.sdo_server,
                                tid,
                                abort_code,
                                asnd_frame.source, // Abort goes back to original sender NodeId
                                asnd_frame.eth_header.source_mac, // Abort goes back to original sender MAC
                            );
                        } else {
                            error!("Cannot send SDO Abort: Could not determine Transaction ID from invalid request.");
                        }
                    }
                }
                // Even if there was an error, we don't proceed with normal frame handling for SDO.
                return NodeAction::NoAction;
            }
        }

        // --- Handle SoC Frame specific logic ---
        if matches!(frame, PowerlinkFrame::Soc(_)) {
            trace!("SoC received at time {}", current_time_us);
            self.last_soc_reception_time_us = current_time_us;
            self.soc_timeout_check_active = true; // Enable timeout check after first SoC
            // Request timer for next SoC check based on cycle time + tolerance
            if let Some(action) = self.request_soc_timeout_check() {
                return action; // Return early as SoC processing is done
            } else {
                // Could not read cycle time or tolerance, proceed without timer request
                warn!("Could not read cycle time/tolerance to set SoC timeout timer.");
            }
        }

        // --- Normal Frame Processing ---

        // 1. Update NMT state machine based on the frame type.
        if let Some(event) = frame.nmt_event() {
            self.nmt_state_machine.process_event(event, &mut self.od);
        }

        // 2. Update DLL state machine based on the frame type.
        if let Some(errors) = self.dll_state_machine.process_event(
            frame.dll_cn_event(),
            self.nmt_state_machine.current_state(),
        ) {
            // If the DLL detects an error (like a lost frame), pass it to the error manager.
            for error in errors {
                warn!("DLL state machine reported error: {:?}", error);
                if self.dll_error_manager.handle_error(error) != NmtAction::None {
                    // Per Table 27, most DLL errors on a CN trigger an NMT state change to PreOp1 [cite: EPSG_301_V-1-5-1_DS-c710608e.pdf, Table 27].
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
            PowerlinkFrame::SoA(frame) => {
                match self.nmt_state() {
                    // Per Table 108, IdentRequest can be handled in PreOp1 and PreOp2.
                    NmtState::NmtPreOperational1 | NmtState::NmtPreOperational2 => {
                        if frame.target_node_id == self.nmt_state_machine.node_id
                            && frame.req_service_id == RequestedServiceId::IdentRequest
                        {
                            Some(payload::build_ident_response(
                                self.mac_address,
                                self.nmt_state_machine.node_id,
                                &self.od,
                                frame,
                            ))
                        } else {
                            None
                        }
                    }
                    _ => None,
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
                    )),
                    _ => None,
                }
            }
            _ => None,
        };

        if let Some(response) = response_frame {
            let mut buf = vec![0u8; 1500];
            if let Ok(size) = response.serialize(&mut buf) {
                buf.truncate(size);
                info!("Sending response frame: {:?}", response);
                trace!("Sending frame bytes ({}): {:?}", size, &buf);
                return NodeAction::SendFrame(buf);
            } else {
                error!("Failed to serialize response frame: {:?}", response);
            }
        }

        NodeAction::NoAction
    }

    /// Calculates the next timer event based on expected SoC arrival.
    /// Returns None if cycle time or tolerance cannot be read.
    fn request_soc_timeout_check(&self) -> Option<NodeAction> {
        let cycle_time_us = self.od.read_u32(OD_IDX_CYCLE_TIME, 0)? as u64;
        // Spec 4.7.8.20: Tolerance is in ns, convert to us
        let tolerance_ns = self.od.read_u32(OD_IDX_LOSS_SOC_TOLERANCE, 0)? as u64;
        let tolerance_us = tolerance_ns / 1000;

        if cycle_time_us == 0 {
            warn!("Cycle Time (0x1006) is zero, cannot check for SoC timeout.");
            return None; // Cannot calculate next check if cycle time is zero
        }

        let next_check_delay_us = cycle_time_us + tolerance_us;
        trace!(
            "Requesting next SoC timeout check in {} us",
            next_check_delay_us
        );
        Some(NodeAction::SetTimer(next_check_delay_us))
    }

    /// Consumes the payload of a PReq frame based on RPDO mapping 0x1400/0x1600.
    fn consume_preq_payload(&mut self, preq: &PReqFrame) {
        self.consume_pdo_payload(
            NodeId(0), // Node ID 0 is reserved for PReq
            &preq.payload,
            preq.pdo_version,
            preq.flags.rd,
        );
    }

    /// Consumes the payload of a PRes frame based on RPDO mapping 0x14xx/0x16xx.
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
        &mut self.od // This now correctly returns the reference with lifetime 's
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
            && buffer.len() > 14
            && buffer[12..14] == crate::types::C_DLL_ETHERTYPE_EPL.to_be_bytes()
        {
            info!("[CN] POWERLINK frame detected in NmtBasicEthernet. Transitioning to NmtPreOperational1.");
            self.nmt_state_machine.process_event(
                NmtEvent::PowerlinkFrameReceived,
                &mut self.od,
            );
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
                warn!("[CN] Could not deserialize frame: {:?}", e);
                if self
                    .dll_error_manager
                    .handle_error(DllError::InvalidFormat)
                    != NmtAction::None
                {
                    self.nmt_state_machine
                        .process_event(NmtEvent::Error, &mut self.od);
                }
                NodeAction::NoAction
            }
        }
    }

    fn tick(&mut self, current_time_us: u64) -> NodeAction {
        let current_nmt_state = self.nmt_state();

        // --- Basic Ethernet Timeout Check ---
        if current_nmt_state == NmtState::NmtNotActive {
            if self.last_soc_reception_time_us == 0 {
                // If it's still 0, no SoC seen yet, start the timer check.
                debug!("No SoC seen, starting BasicEthernet timeout check.");
                self.last_soc_reception_time_us = current_time_us;
                let timeout_duration_us = self.nmt_state_machine.basic_ethernet_timeout as u64;
                return NodeAction::SetTimer(timeout_duration_us);
            }

            let timeout_duration_us = self.nmt_state_machine.basic_ethernet_timeout as u64;
            if current_time_us.saturating_sub(self.last_soc_reception_time_us)
                >= timeout_duration_us
            {
                // Timeout expired without seeing a SoC, transition to BasicEthernet
                // [cite: EPSG_301_V-1-5-1_DS-c710608e.pdf, Section 7.1.4.1.1].
                warn!("BasicEthernet timeout expired. Transitioning state.");
                self.nmt_state_machine
                    .process_event(NmtEvent::Timeout, &mut self.od);
                self.last_soc_reception_time_us = 0; // Reset timer flag
                self.soc_timeout_check_active = false; // Disable SoC check in BasicEthernet
                return NodeAction::NoAction; // No further timer needed for this mode
            } else {
                // Timeout not yet reached, request another check later.
                let remaining_time = timeout_duration_us.saturating_sub(
                    current_time_us.saturating_sub(self.last_soc_reception_time_us),
                );
                trace!(
                    "BasicEthernet timeout check pending, next check in {} us",
                    remaining_time
                );
                return NodeAction::SetTimer(remaining_time);
            }
        }

        // --- SoC Timeout Check (only in cyclic states and if activated) ---
        if self.soc_timeout_check_active
            && matches!(
                current_nmt_state,
                NmtState::NmtPreOperational1 // Check during PreOp1 as well
                    | NmtState::NmtPreOperational2
                    | NmtState::NmtReadyToOperate
                    | NmtState::NmtOperational
                    | NmtState::NmtCsStopped
            )
        {
            if let (Some(cycle_time_us), Some(tolerance_ns)) = (
                self.od.read_u32(OD_IDX_CYCLE_TIME, 0).map(|v| v as u64),
                self.od
                    .read_u32(OD_IDX_LOSS_SOC_TOLERANCE, 0)
                    .map(|v| v as u64),
            ) {
                if cycle_time_us > 0 {
                    let tolerance_us = tolerance_ns / 1000;
                    let expected_soc_time_us = self
                        .last_soc_reception_time_us
                        .saturating_add(cycle_time_us);
                    let timeout_threshold_us = expected_soc_time_us.saturating_add(tolerance_us);

                    if current_time_us >= timeout_threshold_us {
                        warn!(
                            "SoC timeout detected! Current: {}, Last SoC: {}, Expected: {}, Threshold: {}",
                            current_time_us, self.last_soc_reception_time_us, expected_soc_time_us, timeout_threshold_us
                        );
                        // --- SoC Timeout Occurred ---
                        // Inform DLL state machine
                        if let Some(errors) = self.dll_state_machine.process_event(
                            DllCsEvent::SocTimeout,
                            current_nmt_state,
                        ) {
                            for error in errors {
                                // Handle resulting DLL errors (e.g., LossOfSoc)
                                if self.dll_error_manager.handle_error(error) != NmtAction::None {
                                    // Trigger NMT error transition if threshold met
                                    self.nmt_state_machine
                                        .process_event(NmtEvent::Error, &mut self.od);
                                    // Stop checking SoC timeout if we reset NMT state
                                    self.soc_timeout_check_active = false;
                                    // Return early after NMT change
                                    return NodeAction::NoAction;
                                }
                            }
                        }
                        // Assume next SoC should have arrived at expected_soc_time_us for next calc
                        self.last_soc_reception_time_us = expected_soc_time_us;
                        // Request timer for the *next* timeout check
                        let next_check_delay = cycle_time_us + tolerance_us;
                        trace!(
                            "SoC timeout processed, requesting next check in {} us",
                            next_check_delay
                        );
                        return NodeAction::SetTimer(next_check_delay);
                    } else {
                        // Timeout not yet reached, request timer for the threshold time.
                        let remaining_time = timeout_threshold_us.saturating_sub(current_time_us);
                        trace!(
                            "SoC timeout check pending, next check in {} us",
                            remaining_time
                        );
                        return NodeAction::SetTimer(remaining_time);
                    }
                } else {
                    // Cycle time is 0, disable check for this cycle.
                    self.soc_timeout_check_active = false;
                    warn!(
                        "Cycle Time (0x1006) is zero, disabling SoC timeout check for this cycle."
                    );
                }
            } else {
                // Could not read OD values, disable check for this cycle.
                self.soc_timeout_check_active = false;
                warn!("Could not read Cycle Time (0x1006) or Tolerance (0x1C14), disabling SoC timeout check for this cycle.");
            }
        } else if !matches!(current_nmt_state, NmtState::NmtNotActive) {
            // If in a cyclic state but check is not active (e.g., after timeout error),
            // reset the time to ensure the check restarts correctly when a SoC is received.
            self.last_soc_reception_time_us = 0;
        } else {
            // If not in NotActive or cyclic state where check is active, reset flag.
            self.soc_timeout_check_active = false;
        }

        // Decrement DLL error counters at the end of the tick
        self.dll_error_manager.on_cycle_complete();

        NodeAction::NoAction // Default if no specific timer requested
    }

    fn nmt_state(&self) -> NmtState {
        self.nmt_state_machine.current_state()
    }
}