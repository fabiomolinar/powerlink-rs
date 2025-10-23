// crates/powerlink-rs/src/frame/poll/mod.rs

//! Defines poll frames like PReq and PRes.

mod preq;
mod pres;

pub use preq::{PReqFlags, PReqFrame};
pub use pres::{PResFlags, PResFrame, PRFlag, RSFlag};