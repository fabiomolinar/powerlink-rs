// crates/powerlink-rs/src/node/mod.rs
pub mod cn;
pub mod mn;
pub mod pdo_handler;

pub use cn::ControlledNode;
use log::{error, info, trace};
pub use mn::ManagingNode;
pub use mn::{CnInfo, CnState, MnContext};
pub use pdo_handler::PdoHandler;

use crate::frame::PowerlinkFrame;
use crate::frame::basic::MacAddress;
use crate::frame::codec::CodecHelpers;
use crate::nmt::states::NmtState;
use crate::od::ObjectDictionary;
use crate::sdo::SdoClient;
use crate::sdo::SdoServer;
#[cfg(feature = "sdo-udp")]
use crate::types::IpAddress;
use crate::{NodeId, PowerlinkError};
use alloc::vec;
use alloc::vec::Vec;

/// Holds state and components common to all POWERLINK node types (MN and CN).
pub struct CoreNodeContext<'s> {
    pub od: ObjectDictionary<'s>,
    pub mac_address: MacAddress,
    pub sdo_server: SdoServer,
    pub sdo_client: SdoClient,
}

impl<'s> CoreNodeContext<'s> {
    /// Queues an SDO request payload to be sent to a specific target node.
    pub fn queue_sdo_request(&mut self, target_node_id: NodeId, payload: Vec<u8>) {
        self.sdo_client.queue_request(target_node_id, payload);
    }
}

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
///
/// The application's main loop is responsible for polling the network interface(s)
/// and passing any received data to the `run_cycle` method.
pub trait Node {
    /// Runs one full cycle of the node's logic, processing any incoming data
    /// and handling internal timers.
    ///
    /// This is the primary function to call in your main application loop.
    /// It handles:
    /// 1. Processing an incoming Ethernet frame (if provided).
    /// 2. Processing an incoming UDP datagram (if provided and feature is enabled).
    /// 3. Ticking the node's internal timers if no network data was processed.
    /// 4. Prioritizing the resulting actions.
    ///
    /// # Arguments
    /// * `ethernet_frame`: A slice containing a received Ethernet frame, or `None`.
    /// * `udp_datagram`: A tuple with the UDP payload, source IP, and source port, or `None`.
    /// * `current_time_us`: The current timestamp.
    ///
    /// # Returns
    /// The single, highest-priority `NodeAction` to be executed.
    #[cfg(feature = "sdo-udp")]
    fn run_cycle(
        &mut self,
        ethernet_frame: Option<&[u8]>,
        udp_datagram: Option<(&[u8], IpAddress, u16)>,
        current_time_us: u64,
    ) -> NodeAction;

    /// Runs one full cycle of the node's logic (version without `sdo-udp` feature).
    #[cfg(not(feature = "sdo-udp"))]
    fn run_cycle(&mut self, ethernet_frame: Option<&[u8]>, current_time_us: u64) -> NodeAction;

    /// Returns the current NMT state of the node.
    fn nmt_state(&self) -> NmtState;

    /// Returns the absolute timestamp (in microseconds) of the next scheduled event.
    ///
    /// This allows the application's main loop to sleep efficiently until the node
    /// needs to be ticked again. Returns `None` if no time-based events are pending.
    fn next_action_time(&self) -> Option<u64>;
}

pub trait NodeContext<'s> {
    fn is_cn(&self) -> bool;
    fn is_mn(&self) -> bool {
        !self.is_cn()
    }
    fn core(&self) -> &CoreNodeContext<'s>;
    fn core_mut(&mut self) -> &mut CoreNodeContext<'s>;
    fn nmt_state_machine(&self) -> &dyn crate::nmt::NmtStateMachine;
}

/// Helper to serialize a PowerlinkFrame (Ethernet) and prepare the NodeAction.
/// This function is now shared by both CN and MN logic.
pub(super) fn serialize_frame_action<'a>(
    frame: PowerlinkFrame,
    context: &impl NodeContext<'a>,
) -> Result<NodeAction, PowerlinkError> {
    let mut buf = vec![0u8; 1518];

    // Use the new helper method to get the header.
    let eth_header = frame.ethernet_header();

    // Check for invalid frame types based on node type.
    if context.is_cn() {
        if !matches!(frame, PowerlinkFrame::PRes(_) | PowerlinkFrame::ASnd(_)) {
            error!(
                "[CN] Attempted to serialize unexpected response frame type: {:?}",
                frame
            );
            return Ok(NodeAction::NoAction);
        }
    } else {
        // is_mn()
        if matches!(frame, PowerlinkFrame::PRes(_)) {
            error!("[MN] Attempted to serialize a PRes frame, which is invalid for an MN.");
            return Ok(NodeAction::NoAction);
        }
    }

    CodecHelpers::serialize_eth_header(eth_header, &mut buf);

    match frame.serialize(&mut buf[14..]) {
        Ok(pl_size) => {
            let total_size = 14 + pl_size;
            // Ethernet minimum frame size is 60 bytes (excluding preamble/FCS)
            // The OS network stack typically handles padding, so we truncate to the actual data size.
            buf.truncate(total_size.max(60));
            if total_size < 60 {
                // Manually zero-pad if needed, though often not necessary
                for i in total_size..60 {
                    buf[i] = 0;
                }
            }
            info!("Sending frame type: {:?} ({} bytes)", frame, buf.len());
            trace!("Sending frame bytes ({}): {:02X?}", buf.len(), &buf);
            Ok(NodeAction::SendFrame(buf))
        }
        Err(e) => {
            error!("Failed to serialize response frame: {:?}", e);
            Err(e)
        }
    }
}
