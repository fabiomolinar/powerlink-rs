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

    fn handle_cn(&self, node: &ControlledNode) -> Option<PowerlinkFrame> {
        // A CN should only respond to PReq or specific SoA frames when it's
        // in a state that is part of the isochronous cycle [cite: EPSG_301_V-1-5-1_DS-c710608e.pdf, Table 108].
        match node.nmt_state() {
            NmtState::NmtPreOperational2
            | NmtState::NmtReadyToOperate
            | NmtState::NmtOperational => {
                // Existing logic for handling frames in a valid state
                match self {
                    PowerlinkFrame::PReq(frame) => Some(node.build_pres_response(frame)),
                    _ => None, // Other frames like SoC don't require a direct response
                }
            }
            // In PreOp1, the node only responds to specific asynchronous invites like IdentRequest.
            NmtState::NmtPreOperational1 => match self {
                PowerlinkFrame::SoA(frame) => {
                    if frame.target_node_id == node.nmt_state_machine.node_id
                        && frame.req_service_id == RequestedServiceId::IdentRequest
                    {
                        Some(node.build_ident_response(frame))
                    } else {
                        None
                    }
                }
                _ => None,
            },
            // In all other states (NotActive, Initialising, etc.), do not respond.
            _ => None,
        }
    }

}
