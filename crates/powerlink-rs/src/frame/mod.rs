//! Defines the structures and logic for the Data Link Layer (DLL) frames.

pub mod basic;
pub mod codec;
pub mod control;
pub mod cs_state_machine;
pub mod error;
pub mod ms_state_machine;
pub mod poll;

pub use basic::{EthernetHeader, MacAddress}; // Added MacAddress
// Make frame types public so other modules (like `node`) can use them.
pub use codec::{Codec, deserialize_frame};
pub use control::{
    ASndFrame, IdentResponsePayload, RequestedServiceId, ServiceId, SoAFrame, SocFrame,
    StaticErrorBitField, StatusResponsePayload,
}; // Added new payload types
pub use cs_state_machine::{DllCsEvent, DllCsStateMachine};
pub use error::{DllError, DllErrorManager, ErrorHandler, NmtAction, NoOpErrorHandler};
pub use ms_state_machine::{DllMsEvent, DllMsStateMachine};
pub use poll::{PRFlag, PReqFrame, PResFrame, RSFlag};

use crate::PowerlinkError;
use crate::nmt::events::NmtEvent;

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
    /// Returns a reference to the inner EthernetHeader.
    pub fn ethernet_header(&self) -> &EthernetHeader {
        match self {
            PowerlinkFrame::Soc(frame) => &frame.eth_header,
            PowerlinkFrame::PReq(frame) => &frame.eth_header,
            PowerlinkFrame::PRes(frame) => &frame.eth_header,
            PowerlinkFrame::SoA(frame) => &frame.eth_header,
            PowerlinkFrame::ASnd(frame) => &frame.eth_header,
        }
    }

    /// Serializes the frame into the provided buffer.
    /// This serializes *only* the POWERLINK frame section (after Eth header).
    pub fn serialize(&self, buffer: &mut [u8]) -> Result<usize, PowerlinkError> {
        match self {
            PowerlinkFrame::Soc(frame) => frame.serialize(buffer),
            PowerlinkFrame::PReq(frame) => frame.serialize(buffer),
            PowerlinkFrame::PRes(frame) => frame.serialize(buffer),
            PowerlinkFrame::SoA(frame) => frame.serialize(buffer),
            PowerlinkFrame::ASnd(frame) => frame.serialize(buffer),
        }
    }

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

    // --- Helper methods for tests and unwrapping ---

    pub fn into_soc(self) -> Result<SocFrame, Self> {
        match self {
            PowerlinkFrame::Soc(frame) => Ok(frame),
            _ => Err(self),
        }
    }

    pub fn into_preq(self) -> Result<PReqFrame, Self> {
        match self {
            PowerlinkFrame::PReq(frame) => Ok(frame),
            _ => Err(self),
        }
    }

    pub fn into_pres(self) -> Result<PResFrame, Self> {
        match self {
            PowerlinkFrame::PRes(frame) => Ok(frame),
            _ => Err(self),
        }
    }

    pub fn into_soa(self) -> Result<SoAFrame, Self> {
        match self {
            PowerlinkFrame::SoA(frame) => Ok(frame),
            _ => Err(self),
        }
    }

    pub fn into_asnd(self) -> Result<ASndFrame, Self> {
        match self {
            PowerlinkFrame::ASnd(frame) => Ok(frame),
            _ => Err(self),
        }
    }
}
