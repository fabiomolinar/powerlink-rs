use super::{handler::FrameHandler, Node, NodeAction};
use crate::frame::basic::MacAddress;
use crate::frame::poll::PResFlags;
use crate::frame::{
    deserialize_frame,
    error::{CnErrorCounters, DllErrorManager, LoggingErrorHandler},
    ASndFrame, Codec, DllCsEvent, DllCsStateMachine, DllError, NmtAction, PowerlinkFrame,
    PReqFrame, PResFrame, PRFlag, RSFlag, ServiceId,
};
use crate::nmt::cn_state_machine::CnNmtStateMachine;
use crate::nmt::state_machine::NmtStateMachine;
use crate::nmt::states::{NmtEvent, NmtState};
use crate::od::{ObjectDictionary, ObjectValue};
use crate::pdo::{PdoMappingEntry, PDOVersion};
use crate::sdo::command::{CommandId, CommandLayerHeader, SdoCommand, Segmentation};
use crate::sdo::sequence::{ReceiveConnState, SendConnState, SequenceLayerHeader};
use crate::sdo::server::SdoServer;
use crate::types::{NodeId, C_ADR_MN_DEF_NODE_ID};
use crate::PowerlinkError;
use alloc::vec;
use alloc::vec::Vec;
use log::{debug, error, info, trace, warn};

// Constants for OD access
const OD_IDX_CYCLE_TIME: u16 = 0x1006;
const OD_IDX_LOSS_SOC_TOLERANCE: u16 = 0x1C14;
const OD_IDX_TPDO_COMM_PARAM_BASE: u16 = 0x1800;
const OD_IDX_TPDO_MAPP_PARAM_BASE: u16 = 0x1A00;
const OD_IDX_RPDO_COMM_PARAM_BASE: u16 = 0x1400;
const OD_IDX_RPDO_MAPP_PARAM_BASE: u16 = 0x1600;
const OD_IDX_PRES_PAYLOAD_LIMIT: u16 = 0x1F98;
const OD_SUBIDX_PRES_PAYLOAD_LIMIT: u8 = 5;
const OD_SUBIDX_PDO_COMM_NODEID: u8 = 1;
const OD_SUBIDX_PDO_COMM_VERSION: u8 = 2;

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
            if asnd_frame.service_id == crate::frame::ServiceId::Sdo {
                debug!("Received SDO/ASnd frame for processing.");
                // Extract SDO Headers first to get Transaction ID for potential abort
                let transaction_id = if asnd_frame.payload.len() >= 4 { // Seq header
                    SequenceLayerHeader::deserialize(&asnd_frame.payload[0..4])
                        .ok() // Ignore deserialization errors here, let SDO server handle payload
                        .and_then(|_seq_header| {
                             if asnd_frame.payload.len() >= 8 { // Cmd header
                                SdoCommand::deserialize(&asnd_frame.payload[4..])
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
                            crate::frame::ServiceId::Sdo,
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
                            return self.build_sdo_abort_response(
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
            PowerlinkFrame::PReq(preq_frame) => {
                self.consume_preq_payload(preq_frame);
            }
            PowerlinkFrame::PRes(pres_frame) => {
                self.consume_pres_payload(pres_frame);
            }
            _ => {} // Other frames do not carry consumer PDOs
        }


        // 4. Delegate response logic to the frame handler.
        if let Some(response_frame) = frame.handle_cn(self) {
            let mut buf = vec![0u8; 1500];
            let serialize_result = match &response_frame {
                PowerlinkFrame::Soc(frame) => frame.serialize(&mut buf),
                PowerlinkFrame::PReq(frame) => frame.serialize(&mut buf),
                PowerlinkFrame::PRes(frame) => frame.serialize(&mut buf),
                PowerlinkFrame::SoA(frame) => frame.serialize(&mut buf),
                PowerlinkFrame::ASnd(frame) => frame.serialize(&mut buf),
            };
            if let Ok(size) = serialize_result {
                buf.truncate(size);
                info!("Sending response frame: {:?}", response_frame);
                trace!("Sending frame bytes ({}): {:?}", size, &buf);
                return NodeAction::SendFrame(buf);
            } else {
                error!("Failed to serialize response frame: {:?}", response_frame);
            }
        }

        NodeAction::NoAction
    }

    /// Builds an `ASnd` frame for the `IdentResponse` service.
    /// This function is typically called by the `FrameHandler` implementation for `SoAFrame`.
    pub(super) fn build_ident_response(&self, soa: &crate::frame::SoAFrame) -> PowerlinkFrame {
        debug!("Building IdentResponse for SoA from node {}", soa.source.0);
        let payload = self.build_ident_response_payload();
        let asnd = ASndFrame::new(
            self.mac_address,
            soa.eth_header.source_mac, // Send back to the MN's MAC
            NodeId(C_ADR_MN_DEF_NODE_ID), // Destination Node ID is MN
            self.nmt_state_machine.node_id,
            crate::frame::ServiceId::IdentResponse,
            payload,
        );
        PowerlinkFrame::ASnd(asnd)
    }

    /// Builds a `PRes` frame in response to being polled by a `PReq`.
    /// This function is typically called by the `FrameHandler` implementation for `PReqFrame`.
    pub(super) fn build_pres_response(&self, _preq: &PReqFrame) -> PowerlinkFrame {
        debug!(
            "Building PRes in response to PReq for node {}",
            self.nmt_state_machine.node_id.0
        );
        
        let nmt_state = self.nmt_state();
        let (payload, pdo_version, payload_is_valid) = 
            match self.build_tpdo_payload() {
                Ok((payload, version)) => (payload, version, true),
                Err(e) => {
                    warn!("Failed to build TPDO payload: {:?}. Sending empty PRes.", e);
                    (Vec::new(), PDOVersion(0), false)
                }
            };

        // Determine RD flag based on NMT state and PDO validity
        // [cite: EPSG_301_V-1-5-1_DS-c710608e.pdf, Section 6.4.4, 6.4.8.2]
        let rd_flag = (nmt_state == NmtState::NmtOperational) && payload_is_valid;

        // TODO: Determine RS and PR flags from SDO/NMT queues
        let rs_flag = RSFlag::new(0); // No pending requests for now
        let pr_flag = PRFlag::Low1; // Default priority

        let flags = PResFlags {
            ms: false, // Assume not multiplexed for now
            en: false, // Exception New (from Error Signaling) - false for now
            rd: rd_flag,
            pr: pr_flag,
            rs: rs_flag,
        };

        let pres = PResFrame::new(
            self.mac_address,
            self.nmt_state_machine.node_id,
            nmt_state,
            flags,
            pdo_version,
            payload,
        );
        PowerlinkFrame::PRes(pres)
    }

    /// Constructs the detailed payload for an `IdentResponse` frame by reading from the OD.
    /// The structure is defined in EPSG DS 301, Section 7.3.3.2.1.
    fn build_ident_response_payload(&self) -> Vec<u8> {
        // Size according to spec: 158 bytes total payload
        let mut payload = vec![0u8; 158];

        // --- Populate fields based on OD values ---

        // Flags (Octet 0-1): PR/RS - Assume none pending for now
        // NMTState (Octet 2)
        payload[2] = self.nmt_state_machine.current_state() as u8;
        // Reserved (Octet 3)
        // EPLVersion (Octet 4) - from 0x1F83/0
        payload[4] = self.od.read_u8(0x1F83, 0).unwrap_or(0);
        // Reserved (Octet 5)
        // FeatureFlags (Octets 6-9) - from 0x1F82/0
        payload[6..10]
            .copy_from_slice(&self.nmt_state_machine.feature_flags.0.to_le_bytes());
        // MTU (Octets 10-11) - from 0x1F98/8 (AsyncMTU_U16)
        payload[10..12].copy_from_slice(&self.od.read_u16(0x1F98, 8).unwrap_or(0).to_le_bytes());
        // PollInSize (Octets 12-13) - from 0x1F98/4 (PreqActPayloadLimit_U16)
        payload[12..14].copy_from_slice(&self.od.read_u16(0x1F98, 4).unwrap_or(0).to_le_bytes());
        // PollOutSize (Octets 14-15) - from 0x1F98/5 (PresActPayloadLimit_U16)
        payload[14..16].copy_from_slice(&self.od.read_u16(0x1F98, 5).unwrap_or(0).to_le_bytes());
        // ResponseTime (Octets 16-19) - from 0x1F98/3 (PresMaxLatency_U32)
        payload[16..20].copy_from_slice(&self.od.read_u32(0x1F98, 3).unwrap_or(0).to_le_bytes());
        // Reserved (Octets 20-21)
        // DeviceType (Octets 22-25) - from 0x1000/0
        payload[22..26].copy_from_slice(&self.od.read_u32(0x1000, 0).unwrap_or(0).to_le_bytes());
        // VendorID (Octets 26-29) - from 0x1018/1
        payload[26..30].copy_from_slice(&self.od.read_u32(0x1018, 1).unwrap_or(0).to_le_bytes());
        // ProductCode (Octets 30-33) - from 0x1018/2
        payload[30..34].copy_from_slice(&self.od.read_u32(0x1018, 2).unwrap_or(0).to_le_bytes());
        // RevisionNumber (Octets 34-37) - from 0x1018/3
        payload[34..38].copy_from_slice(&self.od.read_u32(0x1018, 3).unwrap_or(0).to_le_bytes());
        // SerialNumber (Octets 38-41) - from 0x1018/4
        payload[38..42].copy_from_slice(&self.od.read_u32(0x1018, 4).unwrap_or(0).to_le_bytes());
        // VendorSpecificExtension1 (Octets 42-49) - Skipped (zeros)
        // VerifyConfigurationDate (Octets 50-53) - from 0x1020/1
        payload[50..54].copy_from_slice(&self.od.read_u32(0x1020, 1).unwrap_or(0).to_le_bytes());
        // VerifyConfigurationTime (Octets 54-57) - from 0x1020/2
        payload[54..58].copy_from_slice(&self.od.read_u32(0x1020, 2).unwrap_or(0).to_le_bytes());
        // ApplicationSwDate (Octets 58-61) - from 0x1F52/1
        payload[58..62].copy_from_slice(&self.od.read_u32(0x1F52, 1).unwrap_or(0).to_le_bytes());
        // ApplicationSwTime (Octets 62-65) - from 0x1F52/2
        payload[62..66].copy_from_slice(&self.od.read_u32(0x1F52, 2).unwrap_or(0).to_le_bytes());
        // IPAddress (Octets 66-69) - from 0x1E40/2
        payload[66..70].copy_from_slice(&self.od.read_u32(0x1E40, 2).unwrap_or(0).to_le_bytes());
        // SubnetMask (Octets 70-73) - from 0x1E40/3
        payload[70..74].copy_from_slice(&self.od.read_u32(0x1E40, 3).unwrap_or(0).to_le_bytes());
        // DefaultGateway (Octets 74-77) - from 0x1E40/5
        payload[74..78].copy_from_slice(&self.od.read_u32(0x1E40, 5).unwrap_or(0).to_le_bytes());
        // HostName (Octets 78-109) - from 0x1F9A/0 (VISIBLE_STRING32)
        if let Some(cow_val) = self.od.read(0x1F9A, 0) {
            if let crate::od::ObjectValue::VisibleString(s) = &*cow_val {
                let bytes = s.as_bytes();
                let len = bytes.len().min(32); // Max 32 bytes for hostname field
                payload[78..78 + len].copy_from_slice(&bytes[..len]);
            }
        }
        // VendorSpecificExtension2 (Octets 110-157) - Skipped (zeros)

        payload
    }

    /// Builds an ASnd frame containing an SDO Abort message.
    fn build_sdo_abort_response(
        &mut self,
        transaction_id: u8,
        abort_code: u32,
        client_node_id: NodeId,
        client_mac: MacAddress,
    ) -> NodeAction {
        error!(
            "Building SDO Abort response (TID: {}, Code: {:#010X}) for Node {}",
            transaction_id, abort_code, client_node_id.0
        );

        // Construct the SDO Abort command
        let abort_command = SdoCommand {
            header: CommandLayerHeader {
                transaction_id,
                is_response: true,
                is_aborted: true,
                segmentation: Segmentation::Expedited,
                command_id: CommandId::Nil, // Command ID irrelevant for abort
                segment_size: 4,            // Size of the abort code
            },
            data_size: None,
            payload: abort_code.to_le_bytes().to_vec(),
        };

        // Construct the Sequence Layer header (state remains Established during abort)
        let seq_header = SequenceLayerHeader {
            receive_sequence_number: self.sdo_server.current_receive_sequence(), // Ack last received
            receive_con: ReceiveConnState::ConnectionValid,
            send_sequence_number: self.sdo_server.next_send_sequence(), // Use next send number
            send_con: SendConnState::ConnectionValid,
        };

        // Serialize SDO Seq + Cmd into SDO payload buffer
        let mut sdo_payload_buf = vec![0u8; 12]; // 4 (Seq) + 8 (Cmd fixed)
        seq_header
            .serialize(&mut sdo_payload_buf[0..4])
            .unwrap_or_else(|e| {
                error!("Failed to serialize SDO Seq header for abort: {:?}", e);
                0
            }); // Should not fail with correct buffer size
        abort_command
            .serialize(&mut sdo_payload_buf[4..])
            .unwrap_or_else(|e| {
                error!("Failed to serialize SDO Cmd header for abort: {:?}", e);
                0
            }); // Should not fail

        // Construct the ASnd frame
        let abort_asnd = ASndFrame::new(
            self.mac_address,
            client_mac,
            client_node_id,
            self.nmt_state_machine.node_id,
            ServiceId::Sdo,
            sdo_payload_buf,
        );

        // Serialize the ASnd frame
        let mut frame_buf = vec![0u8; 1500];
        match abort_asnd.serialize(&mut frame_buf) {
            Ok(size) => {
                frame_buf.truncate(size);
                warn!("Sending SDO Abort frame ({} bytes).", size);
                NodeAction::SendFrame(frame_buf)
            }
            Err(e) => {
                error!("Failed to serialize SDO Abort ASnd frame: {:?}", e);
                NodeAction::NoAction
            }
        }
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

    /// Reads RPDO mappings for a given source Node ID (0 for PReq) and writes
    /// data from the payload into the Object Dictionary.
    fn consume_pdo_payload(
        &mut self,
        source_node_id: NodeId,
        payload: &[u8],
        received_version: PDOVersion,
        is_ready: bool,
    ) {
        if !is_ready {
            trace!("Ignoring PDO payload, RD flag is not set.");
            return; // Data is not valid
        }

        if self.nmt_state() != NmtState::NmtOperational {
            trace!("Ignoring PDO payload, NMT state is not Operational.");
            return; // Per spec, only consume in Operational
        }

        // Find the correct mapping for this source node
        // TODO: This is a linear scan. For performance, a BTreeMap<NodeId, u16>
        // could cache this lookup (e.g., NodeId(0) -> 0x1400, NodeId(5) -> 0x1401).
        let mut mapping_index = None;
        for i in 0..256 {
            let comm_param_index = OD_IDX_RPDO_COMM_PARAM_BASE + i as u16;
            if let Some(node_id_val) =
                self.od.read_u8(comm_param_index, OD_SUBIDX_PDO_COMM_NODEID)
            {
                if node_id_val == source_node_id.0 {
                    // Found the correct communication parameter object
                    // Check mapping version
                    let expected_version = self
                        .od
                        .read_u8(comm_param_index, OD_SUBIDX_PDO_COMM_VERSION)
                        .unwrap_or(0);
                    if expected_version != 0 && received_version.0 != expected_version {
                        warn!(
                            "PDO version mismatch for source Node {}. Expected {}, got {}. Ignoring payload.",
                            source_node_id.0, expected_version, received_version.0
                        );
                        // TODO: Log E_PDO_MAP_VERS (6.4.8.1.1)
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
                trace!("No RPDO mapping found for source Node {}.", source_node_id.0);
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
                            self.apply_rpdo_mapping_entry(&entry, payload);
                        }
                    }
                }
            }
        }
    }

    /// Helper for `consume_pdo_payload` to apply a single mapping entry.
    fn apply_rpdo_mapping_entry(&mut self, entry: &PdoMappingEntry, payload: &[u8]) {
        // Get byte-aligned offset and length
        let (Some(offset), Some(length)) = (entry.byte_offset(), entry.byte_length()) else {
            warn!("Bit-level PDO mapping is not supported. Index: {}, SubIndex: {}.", entry.index, entry.sub_index);
            return;
        };

        // Check payload bounds
        if payload.len() < offset + length {
            warn!(
                "RPDO mapping for 0x{:04X}/{} is out of bounds. Payload size: {}, expected at least {}.",
                entry.index, entry.sub_index, payload.len(), offset + length
            );
            // TODO: Log E_PDO_SHORT_RX (6.4.8.1.2)
            return;
        }

        // Get the slice of data from the payload
        let data_slice = &payload[offset..offset + length];

        // Get a type template from the OD to deserialize against
        let Some(type_template) = self.od.read(entry.index, entry.sub_index) else {
            warn!("RPDO mapping for 0x{:04X}/{} failed: OD entry not found.", entry.index, entry.sub_index);
            return;
        };
        
        // Deserialize and write to OD
        match ObjectValue::deserialize(data_slice, &type_template) {
            Ok(value) => {
                trace!(
                    "Applying RPDO: Writing {:?} to 0x{:04X}/{}",
                    value,
                    entry.index,
                    entry.sub_index
                );
                // Use write_internal to bypass read-only checks (PDO writes are allowed)
                if let Err(e) = self.od.write_internal(entry.index, entry.sub_index, value, false) {
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

    /// Builds the payload for a TPDO (PRes) frame.
    fn build_tpdo_payload(&self) -> Result<(Vec<u8>, PDOVersion), PowerlinkError> {
        let comm_param_index = OD_IDX_TPDO_COMM_PARAM_BASE; // Default PRes is 0x1800
        let mapping_index = OD_IDX_TPDO_MAPP_PARAM_BASE; // Default PRes is 0x1A00

        // 1. Get Mapping Version
        let pdo_version = PDOVersion(
            self.od
                .read_u8(comm_param_index, OD_SUBIDX_PDO_COMM_VERSION)
                .unwrap_or(0),
        );

        // 2. Get Payload Limit
        let payload_limit = self
            .od
            .read_u16(
                OD_IDX_PRES_PAYLOAD_LIMIT,
                OD_SUBIDX_PRES_PAYLOAD_LIMIT,
            )
            .unwrap_or(0) as usize;
        
        let mut payload = vec![0u8; payload_limit.min(1490)]; // Pre-allocate buffer
        let mut max_offset_len = 0;

        // 3. Iterate mapping and fill payload
        if let Some(mapping_cow) = self.od.read(mapping_index, 0) {
            if let ObjectValue::Unsigned8(num_entries) = *mapping_cow {
                for i in 1..=num_entries {
                    let Some(entry_cow) = self.od.read(mapping_index, i) else { continue };
                    let ObjectValue::Unsigned64(raw_mapping) = *entry_cow else { continue };
                    
                    let entry = PdoMappingEntry::from_u64(raw_mapping);

                    let (Some(offset), Some(length)) = (entry.byte_offset(), entry.byte_length()) else {
                        warn!("Bit-level TPDO mapping not supported for 0x{:04X}/{}", entry.index, entry.sub_index);
                        continue;
                    };

                    let end_pos = offset + length;
                    if end_pos > payload.len() {
                         warn!(
                            "TPDO mapping for 0x{:04X}/{} exceeds payload limit {}. Required: {} bytes.",
                            entry.index, entry.sub_index, payload.len(), end_pos
                        );
                        return Err(PowerlinkError::ValidationError("PDO mapping exceeds payload limit"));
                    }
                    if end_pos > max_offset_len {
                        max_offset_len = end_pos;
                    }

                    // Read from OD
                    let Some(value_cow) = self.od.read(entry.index, entry.sub_index) else {
                        warn!("TPDO mapping for 0x{:04X}/{} failed: OD entry not found.", entry.index, entry.sub_index);
                        // Write zeros to the payload for this entry
                        payload[offset..end_pos].fill(0);
                        continue;
                    };

                    // Serialize and copy to payload
                    let serialized_data = value_cow.serialize();
                    if serialized_data.len() != length {
                        warn!(
                            "TPDO mapping for 0x{:04X}/{} length mismatch. Mapped: {} bytes, Object: {} bytes.",
                            entry.index, entry.sub_index, length, serialized_data.len()
                        );
                        // Truncate or pad, for now just copy the valid part
                        let copy_len = serialized_data.len().min(length);
                        payload[offset..offset+copy_len].copy_from_slice(&serialized_data[..copy_len]);
                    } else {
                        payload[offset..end_pos].copy_from_slice(&serialized_data);
                    }
                     trace!("Applied TPDO: Read {:?} from 0x{:04X}/{}", value_cow, entry.index, entry.sub_index);
                }
            }
        }
        
        // Truncate payload to the minimum required size
        payload.truncate(max_offset_len);
        Ok((payload, pdo_version))
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

