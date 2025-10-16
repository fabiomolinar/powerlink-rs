use super::cn::ControlledNode;
use crate::frame::{
    DllCsEvent, PowerlinkFrame, RequestedServiceId,
};
use crate::nmt::states::NmtEvent;

/// A trait to handle the specific logic for each POWERLINK frame type.
pub trait FrameHandler {
    /// Determines the DLL event associated with this frame.
    fn dll_event(&self) -> DllCsEvent;

    /// Determines the NMT event associated with this frame, if any.
    fn nmt_event(&self) -> Option<NmtEvent>;

    /// Processes the frame within the context of a `ControlledNode` and
    /// builds a response frame if required.
    fn handle_cn(&self, node: &ControlledNode) -> Option<PowerlinkFrame>;
}

impl FrameHandler for PowerlinkFrame {
    fn dll_event(&self) -> DllCsEvent {
        match self {
            PowerlinkFrame::Soc(_) => DllCsEvent::Soc,
            PowerlinkFrame::PReq(_) => DllCsEvent::Preq,
            PowerlinkFrame::PRes(_) => DllCsEvent::Pres,
            PowerlinkFrame::SoA(_) => DllCsEvent::Soa,
            PowerlinkFrame::ASnd(_) => DllCsEvent::Asnd,
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
        match self {
            PowerlinkFrame::SoA(frame) => {
                if frame.target_node_id == node.nmt_state_machine.node_id
                    && frame.req_service_id == RequestedServiceId::IdentRequest
                {
                    Some(node.build_ident_response(frame))
                } else {
                    None
                }
            }
            PowerlinkFrame::PReq(frame) => Some(node.build_pres_response(frame)),
            _ => None,
        }
    }
}
