// crates/powerlink-rs/src/sdo/client.rs
use crate::frame::PRFlag;
use alloc::vec::Vec;

/// Manages the client-side SDO logic, primarily for queueing
/// outgoing requests.
#[derive(Default)]
pub struct SdoClient {
    /// Queue of SDO request payloads to be sent.
    pending_client_requests: Vec<Vec<u8>>,
}

impl SdoClient {
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues an SDO request payload to be sent.
    pub fn queue_request(&mut self, payload: Vec<u8>) {
        self.pending_client_requests.push(payload);
    }

    /// Retrieves and removes the next pending client request from the queue.
    pub fn pop_pending_request(&mut self) -> Option<Vec<u8>> {
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
            // SDO via ASnd uses PRIO_GENERIC_REQUEST.
            (count.min(7) as u8, PRFlag::PrioGenericRequest)
        } else {
            (0, PRFlag::default())
        }
    }
}
