pub mod cn;
pub mod handler;

use crate::nmt::states::NmtState;
use alloc::vec::Vec;

/// Represents the possible actions a POWERLINK node might need to perform
/// in response to an event or a tick.
#[derive(Debug)]
pub enum NodeAction {
    /// The node needs to send a frame over the network.
    SendFrame(Vec<u8>),
    /// The node needs a timer to be set for the specified duration in microseconds.
    /// When the timer expires, the application should call `tick()`.
    SetTimer(u64),
    /// No immediate action is required.
    NoAction,
}

/// A trait that defines the common interface for all POWERLINK nodes (MN and CN).
pub trait Node {
    /// Processes a raw byte buffer received from the network.
    fn process_raw_frame(&mut self, buffer: &[u8]) -> NodeAction;
    /// Called periodically by the application to handle time-based events, like timeouts.
    fn tick(&mut self) -> NodeAction;
    /// Returns the current NMT state of the node.
    fn nmt_state(&self) -> NmtState;
}

