//! Defines the structures and logic for the Data Link Layer (DLL) frames.

pub mod basic;
pub mod control;
pub mod poll;
pub mod cs_state_machine;
pub mod ms_state_machine;
pub mod error;
pub mod codec;

pub use basic::EthernetHeader;
pub use control::{SocFrame, SoAFrame, RequestedServiceId, ASndFrame, ServiceId};
pub use poll::{PReqFrame, PResFrame, RSFlag, PRFlag};
pub use cs_state_machine::{DllCsStateMachine, DllCsEvent};
pub use ms_state_machine::{DllMsStateMachine, DllMsEvent};
pub use error::{DllError, DllErrorManager, ErrorHandler, NoOpErrorHandler, NmtAction};
pub use codec::{Codec, deserialize_frame};

/// Represents any POWERLINK frame
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PowerlinkFrame {
    Soc(SocFrame),
    PReq(PReqFrame),
    PRes(PResFrame),
    SoA(SoAFrame),
    ASnd(ASndFrame),
}