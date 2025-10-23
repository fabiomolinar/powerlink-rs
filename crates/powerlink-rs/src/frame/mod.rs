//! Defines the structures and logic for the Data Link Layer (DLL) frames.

pub mod basic;
pub mod control;
pub mod poll;
pub mod cs_state_machine;
pub mod ms_state_machine;
pub mod error;
pub mod codec;

pub use basic::EthernetHeader;
// Make frame types public so other modules (like `node`) can use them.
pub use control::{ASndFrame, ServiceId, SoAFrame, RequestedServiceId, SocFrame};
pub use poll::{PReqFrame, PResFrame, RSFlag, PRFlag};
pub use cs_state_machine::{DllCsStateMachine, DllCsEvent};
pub use ms_state_machine::{DllMsStateMachine, DllMsEvent};
pub use error::{DllError, DllErrorManager, ErrorHandler, NoOpErrorHandler, NmtAction};
pub use codec::{Codec, deserialize_frame};

use crate::nmt::states::NmtEvent;

/// Represents any POWERLINK frame
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PowerlinkFrame {
    Soc(SocFrame),
    PReq(PReqFrame),
    PRes(PResFrame),
    SoA(SoAFrame),
    ASnd(ASndFrame),
}

impl PowerlinkFrame {
    /// Determines the DLL event for a Controlled Node.
    pub fn dll_cn_event(&self) -> DllCsEvent {
        match self {
            PowerlinkFrame::Soc(_) => DllCsEvent::Soc,
            PowerlinkFrame::PReq(_) => DllCsEvent::Preq,
            PowerlinkFrame::PRes(_) => DllCsEvent::Pres,
            PowerlinkFrame::SoA(_) => DllCsEvent::Soa,
            PowerlinkFrame::ASnd(_) => DllCsEvent::Asnd,
        }
    }

    /// Determines the DLL event for a Managing Node.
    pub fn dll_mn_event(&self) -> DllMsEvent {
        match self {
            PowerlinkFrame::PRes(_) => DllMsEvent::Pres,
            PowerlinkFrame::ASnd(_) => DllMsEvent::Asnd,
            // Other frames are sent by the MN, not received by its DLL state machine.
            _ => DllMsEvent::Asnd, // Placeholder/Default
        }
    }

    /// Determines the NMT event associated with this frame, if any.
    pub fn nmt_event(&self) -> Option<NmtEvent> {
        match self {
            PowerlinkFrame::Soc(_) => Some(NmtEvent::SocReceived),
            PowerlinkFrame::SoA(_) => Some(NmtEvent::SocSoAReceived),
            // PReq/PRes are part of the cycle, not NMT-level events themselves
            _ => None,
        }
    }
}
