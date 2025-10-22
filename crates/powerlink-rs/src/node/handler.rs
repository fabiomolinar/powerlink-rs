use super::{cn::ControlledNode};
use crate::frame::{
    DllCsEvent, DllMsEvent, PowerlinkFrame, RequestedServiceId,
};
use crate::nmt::states::{NmtEvent, NmtState};
use crate::{Node};

/// A trait to handle the specific logic for each POWERLINK frame type.
pub trait FrameHandler {
    /// Determines the DLL event for a Controlled Node.
    fn dll_cn_event(&self) -> DllCsEvent;

    /// Determines the DLL event for a Managing Node.
    fn dll_mn_event(&self) -> DllMsEvent;

    /// Determines the NMT event associated with this frame, if any.
    fn nmt_event(&self) -> Option<NmtEvent>;

    /// Processes the frame within the context of a `ControlledNode` and
    /// builds a response frame if required.
    fn handle_cn(&self, node: &ControlledNode) -> Option<PowerlinkFrame>;
}

impl FrameHandler for PowerlinkFrame {
    fn dll_cn_event(&self) -> DllCsEvent {
        match self {
            PowerlinkFrame::Soc(_) => DllCsEvent::Soc,
            PowerlinkFrame::PReq(_) => DllCsEvent::Preq,
            PowerlinkFrame::PRes(_) => DllCsEvent::Pres,
            PowerlinkFrame::SoA(_) => DllCsEvent::Soa,
            PowerlinkFrame::ASnd(_) => DllCsEvent::Asnd,
        }
    }

    fn dll_mn_event(&self) -> DllMsEvent {
        match self {
            PowerlinkFrame::PRes(_) => DllMsEvent::Pres,
            PowerlinkFrame::ASnd(_) => DllMsEvent::Asnd,
            // Other frames are sent by the MN, not received.
            _ => DllMsEvent::Asnd, // Placeholder
        }
    }

    fn nmt_event(&self) -> Option<NmtEvent> {
        match self {
            PowerlinkFrame::Soc(_) => Some(NmtEvent::SocReceived),
            PowerlinkFrame::SoA(_) => Some(NmtEvent::SocSoAReceived),
            _ => None,
        }
    }

    /// Processes the frame within the context of a `ControlledNode` and
    /// builds a response frame if required.
    fn handle_cn(&self, node: &ControlledNode) -> Option<PowerlinkFrame> {
        match self {
            // Handle SoA frames, specifically IdentRequest.
            PowerlinkFrame::SoA(frame) => {
                match node.nmt_state() {
                    // Per Table 108, IdentRequest can be handled in PreOp1 and PreOp2.
                    NmtState::NmtPreOperational1 | NmtState::NmtPreOperational2 => {
                        if frame.target_node_id == node.nmt_state_machine.node_id
                            && frame.req_service_id == RequestedServiceId::IdentRequest
                        {
                            Some(node.build_ident_response(frame))
                        } else {
                            None
                        }
                    }
                    // In other states, a CN does not respond to SoA.
                    _ => None,
                }
            }
            // Handle PReq frames.
            PowerlinkFrame::PReq(frame) => {
                match node.nmt_state() {
                    // A CN only responds to PReq when in isochronous states.
                    NmtState::NmtPreOperational2
                    | NmtState::NmtReadyToOperate
                    | NmtState::NmtOperational => Some(node.build_pres_response(frame)),
                    // In other states, a CN does not respond to PReq.
                    _ => None,
                }
            }
            // Other frames like SoC, PRes, ASnd do not require a direct response from a CN
            // in this handler logic. SDO/ASnd is handled earlier in the node.
            _ => None,
        }
    }
}

