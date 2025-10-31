use crate::node::NodeAction;
use crate::sdo::command::SdoCommand;
use crate::sdo::sequence::SequenceLayerHeader;
use crate::sdo::server::SdoClientInfo;
use crate::PowerlinkError;

/// Encapsulates the data required to construct an SDO response,
/// making the SdoServer transport-agnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdoResponseData {
    /// Information about the client to which the response should be sent.
    pub client_info: SdoClientInfo,
    /// The sequence layer header for the response.
    pub seq_header: SequenceLayerHeader,
    /// The command layer data for the response.
    pub command: SdoCommand,
}

/// A trait for abstracting the SDO transport mechanism (e.g., ASnd or UDP).
pub trait SdoTransport {
    /// Builds a transport-specific `NodeAction` from generic SDO response data.
    ///
    /// # Arguments
    /// * `data` - The SDO response data to be formatted.
    ///
    /// # Returns
    /// A `NodeAction` (e.g., `SendFrame` or `SendUdp`) ready to be executed by the node.
    fn build_response(&self, data: SdoResponseData) -> Result<NodeAction, PowerlinkError>;
}