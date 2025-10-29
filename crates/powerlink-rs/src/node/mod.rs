// crates/powerlink-rs/src/node/mod.rs

pub mod cn;
pub mod mn;
pub mod pdo_handler;

pub use cn::ControlledNode;
pub use mn::ManagingNode;
pub use pdo_handler::PdoHandler;

#[cfg(feature = "sdo-udp")]
use crate::types::IpAddress;
use crate::nmt::states::NmtState;
use alloc::vec::Vec;

/// Represents the possible actions a POWERLINK node might need to perform
/// in response to an event or a tick.
#[derive(Debug, PartialEq, Eq)]
pub enum NodeAction {
    /// The node needs to send a raw Ethernet frame over the network.
    SendFrame(Vec<u8>),
    /// The node needs to send a UDP datagram.
    #[cfg(feature = "sdo-udp")]
    SendUdp {
        dest_ip: IpAddress,
        dest_port: u16,
        data: Vec<u8>,
    },
    /// No immediate network action is required.
    NoAction,
}

/// A trait that defines the common interface for all POWERLINK nodes (MN and CN).
pub trait Node {
    /// Processes a raw byte buffer received from the network at a specific time.
    /// This buffer could contain an Ethernet frame or a UDP datagram.
    fn process_raw_frame(&mut self, buffer: &[u8], current_time_us: u64) -> NodeAction;

    /// Called periodically by the application to handle time-based events, like timeouts.
    /// The application is responsible for calling this method, ideally when a deadline
    /// returned by `next_action_time` has been reached.
    fn tick(&mut self, current_time_us: u64) -> NodeAction;

    /// Returns the current NMT state of the node.
    fn nmt_state(&self) -> NmtState;

    /// Returns the absolute timestamp (in microseconds) of the next scheduled event.
    ///
    /// This allows the application's main loop to sleep efficiently until the node
    /// needs to be ticked again. Returns `None` if no time-based events are pending.
    fn next_action_time(&self) -> Option<u64>;
}

