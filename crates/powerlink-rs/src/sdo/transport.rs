use crate::PowerlinkError;
use crate::frame::{ASndFrame, PowerlinkFrame, ServiceId};
use crate::node::{NodeAction, NodeContext, serialize_frame_action};
use crate::sdo::asnd::serialize_sdo_asnd_payload;
use crate::sdo::command::SdoCommand;
use crate::sdo::sequence::SequenceLayerHeader;
use crate::sdo::server::SdoClientInfo;
#[cfg(feature = "sdo-udp")]
use crate::sdo::udp::serialize_sdo_udp_payload;
use log::info;

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
    /// * `context` - The node's context, providing access to node-specific info like MAC address.
    ///
    /// # Returns
    /// A `NodeAction` (e.g., `SendFrame` or `SendUdp`) ready to be executed by the node.
    fn build_response<'a>(
        &self,
        data: SdoResponseData,
        context: &impl NodeContext<'a>,
    ) -> Result<NodeAction, PowerlinkError>;
}

/// An SDO transport implementation for ASnd (Layer 2).
pub struct AsndTransport;

impl SdoTransport for AsndTransport {
    fn build_response<'a>(
        &self,
        data: SdoResponseData,
        context: &impl NodeContext<'a>,
    ) -> Result<NodeAction, PowerlinkError> {
        let (source_node_id, source_mac) = match data.client_info {
            SdoClientInfo::Asnd {
                source_node_id,
                source_mac,
            } => (source_node_id, source_mac),
            #[cfg(feature = "sdo-udp")]
            SdoClientInfo::Udp { .. } => {
                return Err(PowerlinkError::InternalError(
                    "Attempted to build ASnd response for UDP client",
                ));
            }
        };

        let sdo_payload = serialize_sdo_asnd_payload(data.seq_header, data.command)?;
        let asnd_frame = ASndFrame::new(
            context.core().mac_address,
            source_mac,
            source_node_id,
            context.nmt_state_machine().node_id(),
            ServiceId::Sdo,
            sdo_payload,
        );

        info!(
            "Building SDO response via ASnd to Node {}",
            source_node_id.0
        );
        serialize_frame_action(PowerlinkFrame::ASnd(asnd_frame), context)
    }
}

/// An SDO transport implementation for UDP/IP.
#[cfg(feature = "sdo-udp")]
pub struct UdpTransport;

#[cfg(feature = "sdo-udp")]
impl SdoTransport for UdpTransport {
    fn build_response<'a>(
        &self,
        data: SdoResponseData,
        _context: &impl NodeContext<'a>, // Context not needed for UDP response
    ) -> Result<NodeAction, PowerlinkError> {
        let (source_ip, source_port) = match data.client_info {
            SdoClientInfo::Udp {
                source_ip,
                source_port,
            } => (source_ip, source_port),
            SdoClientInfo::Asnd { .. } => {
                return Err(PowerlinkError::InternalError(
                    "Attempted to build UDP response for ASnd client",
                ));
            }
        };

        let mut udp_buffer = vec![0u8; 1500]; // Standard MTU size
        let udp_payload_len =
            serialize_sdo_udp_payload(data.seq_header, data.command, &mut udp_buffer)?;
        udp_buffer.truncate(udp_payload_len);

        info!(
            "Building SDO response via UDP to {}:{}",
            core::net::Ipv4Addr::from(source_ip),
            source_port
        );

        Ok(NodeAction::SendUdp {
            dest_ip: source_ip,
            dest_port: source_port,
            data: udp_buffer,
        })
    }
}
