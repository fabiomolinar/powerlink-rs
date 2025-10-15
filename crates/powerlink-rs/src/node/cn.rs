// In crates/powerlink-rs/src/node/cn.rs

use crate::frame::basic::MacAddress;
use crate::frame::{
    ASndFrame, DllCsEvent, DllCsStateMachine, PowerlinkFrame, RequestedServiceId, ServiceId,
};
use crate::nmt::cn_state_machine::CnNmtStateMachine;
use crate::nmt::states::NmtEvent;
use crate::od::{ObjectDictionary, ObjectValue};
use crate::types::{NodeId, C_ADR_MN_DEF_NODE_ID};
use crate::PowerlinkError;
use alloc::borrow::Cow;
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

    /// The main entry point for processing all incoming POWERLINK frames.
    ///
    /// This method drives the DLL and NMT state machines and generates a response
    /// frame if required by the protocol.
    pub fn process_frame(&mut self, frame: PowerlinkFrame) -> Option<PowerlinkFrame> {
        // 1. Map the incoming frame to a DLL event.
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

        // 5. Handle application-level logic based on the frame content.
        match frame {
            PowerlinkFrame::SoA(soa_frame) => {
                // Check if this SoA frame is an IdentRequest addressed to us.
                if soa_frame.target_node_id == self.nmt_state_machine.node_id
                    && soa_frame.req_service_id == RequestedServiceId::IdentRequest
                {
                    // If so, build and return an IdentResponse.
                    return Some(self.build_ident_response(&soa_frame));
                }
            }
            PowerlinkFrame::PReq(preq_frame) => {
                // If we are polled with a PReq, we must respond with a PRes.
                return Some(self.build_pres_response(&preq_frame));
            }
            _ => {
                // Other frames are processed by the state machines but don't require
                // an immediate, direct response from this layer.
            }
        }

        None
    }

    /// Builds an ASnd frame for the IdentResponse service.
    fn build_ident_response(&self, soa: &crate::frame::SoAFrame) -> PowerlinkFrame {
        let payload = self.build_ident_response_payload();
        let asnd = ASndFrame::new(
            self.mac_address,
            soa.eth_header.source_mac, // Respond to the MN's MAC
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
            Default::default(), // Default flags
            crate::pdo::PDOVersion(0),
            payload,
        );
        PowerlinkFrame::PRes(pres)
    }

    /// Constructs the detailed payload for an IdentResponse frame by reading from the OD.
    /// The structure is defined in EPSG DS 301, Section 7.3.3.2.1.
    fn build_ident_response_payload(&self) -> Vec<u8> {
        let mut payload = vec![0u8; 110]; // Minimum size for IdentResponse

        // Helper to read a u32 from OD or default to 0.
        let read_u32 = |index, sub_index| {
            self.od
                .read(index, sub_index)
                .and_then(|cow| {
                    // Dereference the Cow to get a reference, then match.
                    if let ObjectValue::Unsigned32(val) = &*cow {
                        Some(*val)
                    } else {
                        None
                    }
                })
                .unwrap_or(0)
        };

        // NMTState (Octet 2)
        payload[2] = self.nmt_state_machine.current_state as u8;

        // EPLVersion (Octet 4)
        if let Some(Cow::Borrowed(ObjectValue::Unsigned8(val))) = self.od.read(0x1F83, 0) {
            payload[4] = *val;
        }

        // FeatureFlags (Octets 6-9)
        payload[6..10].copy_from_slice(&self.nmt_state_machine.feature_flags.0.to_le_bytes());

        // MTU (Octets 10-11)
        if let Some(cow) = self.od.read(0x1F98, 8) {
            if let ObjectValue::Unsigned16(val) = &*cow {
                payload[10..12].copy_from_slice(&val.to_le_bytes());
            }
        }

        // DeviceType (Octets 22-25) - from 0x1000
        payload[22..26].copy_from_slice(&read_u32(0x1000, 0).to_le_bytes());

        // Identity Object (Octets 26-41) - from 0x1018
        payload[26..30].copy_from_slice(&read_u32(0x1018, 1).to_le_bytes()); // VendorID
        payload[30..34].copy_from_slice(&read_u32(0x1018, 2).to_le_bytes()); // ProductCode
        payload[34..38].copy_from_slice(&read_u32(0x1018, 3).to_le_bytes()); // RevisionNo
        payload[38..42].copy_from_slice(&read_u32(0x1018, 4).to_le_bytes()); // SerialNo

        // Other fields like IPAddress, HostName etc. would be populated similarly.

        payload
    }
}