use crate::frame::basic::MacAddress;
use crate::sdo::command::{DefaultSdoHandler, SdoCommandHandler};
use crate::sdo::sequence_handler::SdoSequenceHandler;
use crate::sdo::state::SdoServerState;
use crate::sdo::transport::SdoResponseData;
#[cfg(feature = "sdo-udp")]
use crate::types::IpAddress;
use crate::{PowerlinkError, od::ObjectDictionary};
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use log::trace;

/// Holds transport-specific information about the SDO client.
/// This must derive Ord to be used as a BTreeMap key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SdoClientInfo {
    /// SDO over ASnd (Layer 2)
    Asnd {
        source_node_id: crate::types::NodeId,
        source_mac: MacAddress,
    },
    /// SDO over UDP/IP (Layer 3/4)
    #[cfg(feature = "sdo-udp")]
    Udp {
        source_ip: IpAddress,
        source_port: u16,
    },
}

/// Manages a single SDO server connection.
/// This server stores the info of the *current* client
/// to handle stateful, multi-frame transfers and timeouts.
pub struct SdoServer {
    /// Map of all active SDO connections, keyed by client info.
    connections: BTreeMap<SdoClientInfo, SdoSequenceHandler>,
    /// Optional handler for vendor-specific or complex commands.
    handler: Box<dyn SdoCommandHandler>,
}

impl SdoServer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a new SdoServer with a custom command handler.
    pub fn with_handler<H: SdoCommandHandler + 'static>(handler: H) -> Self {
        Self {
            handler: Box::new(handler),
            connections: BTreeMap::new(),
        }
    }

    /// Returns the absolute timestamp of the next SDO timeout, if any.
    pub fn next_action_time(&self) -> Option<u64> {
        // Iterate over all handlers and find the minimum Some(deadline)
        self.connections
            .values()
            .filter_map(|handler| match handler.state() {
                SdoServerState::SegmentedUpload(state) => state.deadline_us,
                SdoServerState::SegmentedDownload(state) => state.deadline_us,
                _ => None,
            })
            .min() // Find the earliest deadline
    }

    /// Handles time-based events for the SDO server, like retransmission timeouts.
    /// Returns a full response tuple if an abort or retransmission frame
    /// needs to be sent.
    pub fn tick(
        &mut self,
        current_time_us: u64,
        od: &ObjectDictionary,
    ) -> Result<Option<SdoResponseData>, PowerlinkError> {
        let mut response_data = None;
        let mut clients_to_prune = Vec::new();

        // Iterate mutably to call tick()
        for (client_info, handler) in self.connections.iter_mut() {
            // Only tick if we haven't already found a response in this cycle
            if response_data.is_none() {
                if let Some(response) = handler.tick(current_time_us, od)? {
                    response_data = Some(response);
                }
            }

            // Check if the handler is now closed (e.g., timed out or aborted)
            if handler.is_closed() {
                clients_to_prune.push(*client_info);
            }
        }

        // Prune closed connections
        for client in clients_to_prune {
            trace!("Pruning closed SDO connection for client: {:?}", client);
            self.connections.remove(&client);
        }

        Ok(response_data)
    }

    /// Processes an incoming SDO request payload (starting *directly* with the Sequence Layer header).
    ///
    /// Returns an `SdoResponseData` struct, which the caller is
    /// responsible for packaging into a transport-specific response.
    pub fn handle_request(
        &mut self,
        request_sdo_payload: &[u8], // Starts with Sequence Layer Header
        client_info: SdoClientInfo, // Pass transport info
        od: &mut ObjectDictionary,
        current_time_us: u64,
    ) -> Result<SdoResponseData, PowerlinkError> {
        if request_sdo_payload.len() < 4 {
            return Err(PowerlinkError::BufferTooShort); // Need at least sequence header
        }
        trace!("Handling SDO request payload: {:?}", request_sdo_payload);

        // 1. Get or create the handler for this client
        let handler = self
            .connections
            .entry(client_info)
            .or_insert_with(|| SdoSequenceHandler::new(client_info));

        // 2. Delegate the entire request handling to the sequence handler
        // We pass self.handler (the SdoCommandHandler) into it.
        let response = handler.handle_request(
            request_sdo_payload,
            od,
            current_time_us,
            self.handler.as_mut(), // Pass the command handler
        )?;

        // 3. Check if the handler is now closed and prune it
        if handler.is_closed() {
            trace!(
                "SDO transfer complete, pruning connection for client: {:?}",
                client_info
            );
            self.connections.remove(&client_info);
        }

        // 4. Return the response
        Ok(response)
    }
    // process_command_layer and abort have been moved to SdoSequenceHandler
    // next_send_sequence and current_receive_sequence are also removed
}

impl Default for SdoServer {
    fn default() -> Self {
        Self {
            connections: BTreeMap::new(),
            handler: Box::new(DefaultSdoHandler),
        }
    }
}
