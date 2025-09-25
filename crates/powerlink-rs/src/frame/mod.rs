//! Defines the structures and logic for the Data Link Layer (DLL) frames.

pub mod basic;
pub mod control;
pub mod poll;
pub mod async;

pub use basic::{EthernetHeader, PowerlinkHeader, PowerlinkFrame};
pub use control::{SocFrame, SoAFrame, RequestedServiceId};
pub use poll::{PReqFrame, PResFrame};

/// Represents a POWERLINK frame
#[derive(Debug, Clone, PartialEq, Eq)]
enum PowerlinkFrame{
    Soc(SocFrame),
    PReq(PReqFrame),
    PRes(PResFrame),
    SoA(SoAFrame),
}