pub mod cn;
pub mod mn;
pub mod pdo_handler;

pub use cn::ControlledNode;
pub use mn::ManagingNode;
pub use pdo_handler::PdoHandler;

use crate::nmt::states::NmtState;
use alloc::vec::Vec;

/// Represents the possible actions a POWERLINK node might need to perform
/// in response to an event or a tick.
#[derive(Debug, PartialEq, Eq)] // Added PartialEq/Eq for easier testing/comparison
pub enum NodeAction {
    /// The node needs to send a frame over the network.
    SendFrame(Vec<u8>),
    /// The node needs a timer to be set for the specified duration relative to the current time (microseconds).
    /// When the timer expires, the application should call `tick()` again with the updated time.
    SetTimer(u64),
    /// No immediate action is required.
    NoAction,
}

/// A trait that defines the common interface for all POWERLINK nodes (MN and CN).
pub trait Node {
    /// Processes a raw byte buffer received from the network at a specific time.
    fn process_raw_frame(&mut self, buffer: &[u8], current_time_us: u64) -> NodeAction;
    /// Called periodically by the application to handle time-based events, like timeouts.
    ///
    /// For a Managing Node, this method drives the entire POWERLINK cycle. For a
    /// Controlled Node, it handles internal timeouts. The application is responsible
    /// for calling this method frequently, ideally triggered by the timers requested
    /// via `NodeAction::SetTimer`.
    fn tick(&mut self, current_time_us: u64) -> NodeAction;
    /// Returns the current NMT state of the node.
    fn nmt_state(&self) -> NmtState;
}

