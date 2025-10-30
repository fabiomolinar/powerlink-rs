// crates/powerlink-rs/src/sdo/client.rs

//! Manages outgoing SDO requests (client-side).

use crate::frame::PRFlag;
use crate::types::NodeId;
use alloc::vec::Vec;

/// Manages a queue of pending SDO requests to be sent by a node.
#[derive(Debug, Default)]
pub struct SdoClient {
    /// Queue of (Target Node ID, SDO Payload)
    pending_client_requests: Vec<(NodeId, Vec<u8>)>,
}

impl SdoClient {
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues an SDO request payload to be sent to a specific target node.
    pub fn queue_request(&mut self, target_node_id: NodeId, payload: Vec<u8>) {
        self.pending_client_requests.push((target_node_id, payload));
    }

    /// Retrieves and removes the next pending client request from the queue.
    pub fn pop_pending_request(&mut self) -> Option<(NodeId, Vec<u8>)> {
        if self.pending_client_requests.is_empty() {
            None
        } else {
            // Treat the Vec as a FIFO queue for simplicity.
            Some(self.pending_client_requests.remove(0))
        }
    }

    /// Checks for pending client (outgoing) requests and returns their count and priority.
    /// This is used to set the RS/PR flags in PRes frames.
    pub fn pending_request_count_and_priority(&self) -> (u8, PRFlag) {
        let count = self.pending_client_requests.len();
        if count > 0 {
            // SDO via ASnd uses PRIO_GENERIC_REQUEST. UDP doesn't use PRes flags.
            // A real implementation would check the priority/transport of each pending request.
            (count.min(7) as u8, PRFlag::PrioGenericRequest)
        } else {
            (0, PRFlag::default())
        }
    }

    /// Checks if the client has pending requests.
    pub fn is_empty(&self) -> bool {
        self.pending_client_requests.is_empty()
    }
}

