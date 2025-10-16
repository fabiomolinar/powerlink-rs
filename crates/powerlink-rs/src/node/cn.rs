use super::{handler::FrameHandler, Node, NodeAction};
use crate::frame::basic::MacAddress;
use crate::frame::{
    deserialize_frame, error::CnErrorCounters, ASndFrame, Codec, DllCsStateMachine, DllError,
    DllErrorManager, NoOpErrorHandler, PowerlinkFrame, PResFrame,
};
use crate::nmt::cn_state_machine::CnNmtStateMachine;
use crate::nmt::states::{NmtEvent, NmtState};
use crate::od::ObjectDictionary;
use crate::types::{NodeId, C_ADR_MN_DEF_NODE_ID};
use crate::PowerlinkError;
use alloc::vec;
use alloc::vec::Vec;

/// Represents a complete POWERLINK Controlled Node (CN).
/// This struct owns and manages all protocol layers and state machines.
pub struct ControlledNode<'s> {
    pub(super) od: ObjectDictionary<'s>,
    pub(super) nmt_state_machine: CnNmtStateMachine,
    dll_state_machine: DllCsStateMachine,
    dll_error_manager: DllErrorManager<CnErrorCounters, NoOpErrorHandler>,
    mac_address: MacAddress,
}

impl<'s> ControlledNode<'s> {
    /// Creates a new Controlled Node.
    ///
    /// The application is responsible for creating and populating the Object Dictionary
    /// with device-specific parameters (e.g., Identity Object 0x1018) before passing
    /// it to this constructor. This function will then validate the OD, initialize it,
    /// and read the necessary configuration to initialize the NMT state machine.
    pub fn new(
        mut od: ObjectDictionary<'s>,
        mac_address: MacAddress,
    ) -> Result<Self, PowerlinkError> {
        // Validate that the application has provided all mandatory objects.
        od.validate_mandatory_objects()?;

        // Initialize the OD: populates protocol objects and either restores
        // defaults or loads from persistent storage.
        od.init()?;
        
        // The NMT state machine's constructor is now fallible because it must
        // read critical parameters from the fully configured OD.
        let nmt_state_machine = CnNmtStateMachine::from_od(&od)?;

        Ok(Self {
            od,
            nmt_state_machine,
            dll_state_machine: DllCsStateMachine::new(),
            dll_error_manager: DllErrorManager::new(CnErrorCounters::new(), NoOpErrorHandler),
            mac_address,
        })
    }

    /// Internal function to process a deserialized `PowerlinkFrame`.
    fn process_frame(&mut self, frame: PowerlinkFrame) -> NodeAction {
        // 1. Update DLL state machine based on the frame type.
        if let Some(errors) = self.dll_state_machine.process_event(
            frame.dll_event(),
            self.nmt_state_machine.current_state,
        ) {
            // If the DLL state machine detects an error, pass it to the error manager.
            for error in errors {
                self.handle_dll_action(error);
            }
        }

        // 2. Update NMT state machine based on the frame type.
        let mut next_action = NodeAction::NoAction;
        if let Some(event) = frame.nmt_event() {
            let old_state = self.nmt_state_machine.current_state;
            self.nmt_state_machine.process_event(event, &mut self.od);
            let new_state = self.nmt_state_machine.current_state;

            // If the state transition is to NotActive, start the Basic Ethernet timeout.
            if old_state != new_state && new_state == NmtState::NmtNotActive {
                 next_action = NodeAction::SetTimer(self.nmt_state_machine.basic_ethernet_timeout as u64);
            }
        }

        // 3. Delegate response logic to the frame handler.
        if let Some(response_frame) = frame.handle_cn(self) {
            let mut buf = vec![0u8; 1500];
            let serialize_result = match response_frame {
                PowerlinkFrame::Soc(frame) => frame.serialize(&mut buf),
                PowerlinkFrame::PReq(frame) => frame.serialize(&mut buf),
                PowerlinkFrame::PRes(frame) => frame.serialize(&mut buf),
                PowerlinkFrame::SoA(frame) => frame.serialize(&mut buf),
                PowerlinkFrame::ASnd(frame) => frame.serialize(&mut buf),
            };
            if let Ok(size) = serialize_result {
                buf.truncate(size);
                return NodeAction::SendFrame(buf);
            }
        }

        next_action
    }

    /// Handles a DllError by passing it to the error manager and processing the resulting NMT action.
    fn handle_dll_action(&mut self, error: DllError) {
        let nmt_action = self.dll_error_manager.handle_error(error);
        match nmt_action {
            crate::frame::NmtAction::ResetCommunication => {
                self.nmt_state_machine.process_event(NmtEvent::Error, &mut self.od);
            }
            // Other actions like ResetNode would be handled here if applicable to CN.
            _ => {}
        }
    }


    /// Builds an `ASnd` frame for the `IdentResponse` service.
    /// This function is typically called by the `FrameHandler` implementation for `SoAFrame`.
    pub(super) fn build_ident_response(&self, soa: &crate::frame::SoAFrame) -> PowerlinkFrame {
        let payload = self.build_ident_response_payload();
        let asnd = ASndFrame::new(
            self.mac_address,
            soa.eth_header.source_mac,
            NodeId(C_ADR_MN_DEF_NODE_ID),
            self.nmt_state_machine.node_id,
            crate::frame::ServiceId::IdentResponse,
            payload,
        );
        PowerlinkFrame::ASnd(asnd)
    }

    /// Builds a `PRes` frame in response to being polled by a `PReq`.
    /// This function is typically called by the `FrameHandler` implementation for `PReqFrame`.
    pub(super) fn build_pres_response(&self, _preq: &crate::frame::PReqFrame) -> PowerlinkFrame {
        let payload = Vec::new();
        let pres = PResFrame::new(
            self.mac_address,
            self.nmt_state_machine.node_id,
            self.nmt_state_machine.current_state,
            Default::default(),
            crate::pdo::PDOVersion(0),
            payload,
        );
        PowerlinkFrame::PRes(pres)
    }

    /// Constructs the detailed payload for an `IdentResponse` frame by reading from the OD.
    /// The structure is defined in EPSG DS 301, Section 7.3.3.2.1.
    fn build_ident_response_payload(&self) -> Vec<u8> {
        let mut payload = vec![0u8; 110]; // Minimum size for IdentResponse

        // NMTState (Octet 2)
        payload[2] = self.nmt_state_machine.current_state as u8;

        // EPLVersion (Octet 4) - from 0x1F83
        if let Some(val) = self.od.read_u8(0x1F83, 0) {
            payload[4] = val;
        }

        // FeatureFlags (Octets 6-9) - from 0x1F82
        payload[6..10].copy_from_slice(&self.nmt_state_machine.feature_flags.0.to_le_bytes());
        
        // MTU (Octets 10-11) - from 0x1F98, sub-index 8
        if let Some(val) = self.od.read_u16(0x1F98, 8) {
            payload[10..12].copy_from_slice(&val.to_le_bytes());
        }

        // DeviceType (Octets 22-25) - from 0x1000
        payload[22..26].copy_from_slice(&self.od.read_u32(0x1000, 0).unwrap_or(0).to_le_bytes());
        
        // Identity Object (Octets 26-41) - from 0x1018
        payload[26..30].copy_from_slice(&self.od.read_u32(0x1018, 1).unwrap_or(0).to_le_bytes()); // VendorID
        payload[30..34].copy_from_slice(&self.od.read_u32(0x1018, 2).unwrap_or(0).to_le_bytes()); // ProductCode
        payload[34..38].copy_from_slice(&self.od.read_u32(0x1018, 3).unwrap_or(0).to_le_bytes()); // RevisionNo
        payload[38..42].copy_from_slice(&self.od.read_u32(0x1018, 4).unwrap_or(0).to_le_bytes()); // SerialNo
        
        // Other fields like IPAddress, HostName etc. would be populated similarly in a full implementation.

        payload
    }
}

impl<'s> Node for ControlledNode<'s> {
    fn process_raw_frame(&mut self, buffer: &[u8]) -> NodeAction {
        match deserialize_frame(buffer) {
            Ok(frame) => self.process_frame(frame),
            Err(e) => {
                // If the frame can't be deserialized, it's a fundamental error.
                if let PowerlinkError::InvalidPlFrame = e {
                    self.handle_dll_action(DllError::InvalidFormat);
                }
                NodeAction::NoAction
            }
        }
    }

    fn tick(&mut self) -> NodeAction {
        // This is called when a timer set by the node expires.
        // Currently, the only timer is for the Basic Ethernet timeout.
        if self.nmt_state() == NmtState::NmtNotActive {
            self.nmt_state_machine.process_event(NmtEvent::Timeout, &mut self.od);
        }
        NodeAction::NoAction
    }

    fn nmt_state(&self) -> NmtState {
        self.nmt_state_machine.current_state
    }
}

