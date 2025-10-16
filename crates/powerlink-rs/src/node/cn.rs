use crate::frame::basic::MacAddress;
use crate::frame::{
    deserialize_frame, ASndFrame, Codec, DllCsEvent, DllCsStateMachine, PowerlinkFrame,
    RequestedServiceId, ServiceId,
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
    od: ObjectDictionary<'s>,
    nmt_state_machine: CnNmtStateMachine,
    dll_state_machine: DllCsStateMachine,
    // Store the local MAC address for building responses.
    mac_address: MacAddress,
}

impl<'s> ControlledNode<'s> {
    /// Creates a new Controlled Node.
    ///
    /// The application is responsible for creating and populating the Object Dictionary
    /// with device-specific parameters (e.g., Identity Object 0x1018) before passing
    /// it to this constructor. This function will then read the necessary configuration
    /// from the OD to initialize the NMT state machine.
    pub fn new(
        od: ObjectDictionary<'s>,
        mac_address: MacAddress,
    ) -> Result<Self, PowerlinkError> {
        // The NMT state machine's constructor is now fallible because it must
        // read critical parameters from the OD provided by the application.
        let nmt_state_machine = CnNmtStateMachine::from_od(&od)?;

        Ok(Self {
            od,
            nmt_state_machine,
            dll_state_machine: DllCsStateMachine::new(),
            mac_address,
        })
    }

    fn process_frame(&mut self, frame: PowerlinkFrame) -> NodeAction {
        let dll_event = match &frame {
            PowerlinkFrame::Soc(_) => DllCsEvent::Soc,
            PowerlinkFrame::PReq(_) => DllCsEvent::Preq,
            PowerlinkFrame::PRes(_) => DllCsEvent::Pres,
            PowerlinkFrame::SoA(_) => DllCsEvent::Soa,
            PowerlinkFrame::ASnd(_) => DllCsEvent::Asnd,
        };

        // 2. Update the DLL state machine. It requires the current NMT state to make decisions.
        if let Some(errors) =
            self.dll_state_machine.process_event(dll_event, self.nmt_state_machine.current_state)
        {
            // 3. If the DLL detects an error (like a lost frame), notify the NMT state machine.
            // Per Table 27, most DLL errors on a CN trigger an NMT state change to PreOp1.
            for _error in errors {
                self.nmt_state_machine.process_event(NmtEvent::Error);
            }
        }

        // 4. Map the incoming frame to a high-level NMT event.
        let nmt_event = match &frame {
            PowerlinkFrame::Soc(_) => Some(NmtEvent::SocReceived),
            PowerlinkFrame::SoA(_) => Some(NmtEvent::SocSoAReceived),
            _ => None,
        };
        if let Some(event) = nmt_event {
            self.nmt_state_machine.process_event(event);
        }

        let response = match frame {
            PowerlinkFrame::SoA(soa_frame) => {
                if soa_frame.target_node_id == self.nmt_state_machine.node_id
                    && soa_frame.req_service_id == RequestedServiceId::IdentRequest
                {
                    Some(self.build_ident_response(&soa_frame))
                } else {
                    None
                }
            }
            PowerlinkFrame::PReq(preq_frame) => Some(self.build_pres_response(&preq_frame)),
            _ => None,
        };

        if let Some(response_frame) = response {
            let mut buf = vec![0u8; 1500];
            if let Ok(size) = response_frame.serialize(&mut buf) {
                buf.truncate(size);
                return NodeAction::SendFrame(buf);
            }
        }

        NodeAction::NoAction
    }

    fn build_ident_response(&self, soa: &crate::frame::SoAFrame) -> PowerlinkFrame {
        let payload = self.build_ident_response_payload();
        let asnd = ASndFrame::new(
            self.mac_address,
            soa.eth_header.source_mac,
            NodeId(C_ADR_MN_DEF_NODE_ID),
            self.nmt_state_machine.node_id,
            ServiceId::IdentResponse,
            payload,
        );
        PowerlinkFrame::ASnd(asnd)
    }

    /// Builds a PRes frame in response to being polled by a PReq.
    fn build_pres_response(&self, _preq: &crate::frame::PReqFrame) -> PowerlinkFrame {
        // TODO: Implement actual PDO payload logic here. For now, it's empty.
        let payload = Vec::new();
        let pres = crate::frame::PResFrame::new(
            self.mac_address,
            self.nmt_state_machine.node_id,
            self.nmt_state_machine.current_state,
            Default::default(),
            crate::pdo::PDOVersion(0),
            payload,
        );
        PowerlinkFrame::PRes(pres)
    }

    /// Constructs the detailed payload for an IdentResponse frame by reading from the OD.
    /// The structure is defined in EPSG DS 301, Section 7.3.3.2.1.
    fn build_ident_response_payload(&self) -> Vec<u8> {
        let mut payload = vec![0u8; 110];
        payload[2] = self.nmt_state_machine.current_state as u8;
        if let Some(val) = self.od.read_u8(0x1F83, 0) {
            payload[4] = val;
        }
        payload[6..10].copy_from_slice(&self.nmt_state_machine.feature_flags.0.to_le_bytes());
        if let Some(val) = self.od.read_u16(0x1F98, 8) {
             payload[10..12].copy_from_slice(&val.to_le_bytes());
        }
        payload[22..26].copy_from_slice(&self.od.read_u32(0x1000, 0).unwrap_or(0).to_le_bytes());
        payload[26..30].copy_from_slice(&self.od.read_u32(0x1018, 1).unwrap_or(0).to_le_bytes());
        payload[30..34].copy_from_slice(&self.od.read_u32(0x1018, 2).unwrap_or(0).to_le_bytes());
        payload[34..38].copy_from_slice(&self.od.read_u32(0x1018, 3).unwrap_or(0).to_le_bytes());
        payload[38..42].copy_from_slice(&self.od.read_u32(0x1018, 4).unwrap_or(0).to_le_bytes());
        payload
    }
}

impl<'s> Node for ControlledNode<'s> {
    fn process_raw_frame(&mut self, buffer: &[u8]) -> NodeAction {
        if let Ok(frame) = deserialize_frame(buffer) {
            self.process_frame(frame)
        } else {
            NodeAction::NoAction
        }
    }

    fn tick(&mut self) -> NodeAction {
        // Future implementation: check for timeouts.
        // If a timeout occurred, inject an NmtEvent::Timeout.
        // self.nmt_state_machine.process_event(NmtEvent::Timeout);
        NodeAction::NoAction
    }

    fn nmt_state(&self) -> NmtState {
        self.nmt_state_machine.current_state
    }
}
