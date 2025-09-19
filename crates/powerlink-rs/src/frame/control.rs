use crate::frame::basic::{EthernetHeader, PowerlinkHeader, MAC_ADDRESS_SIZE};
use crate::types::{NodeId, UNSIGNED16, UNSIGNED32};

// --- Start of Cycle (SoC) ---

/// Represents a complete SoC frame (MN multicast control message).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SocFrame {
    pub eth_header: EthernetHeader,
    pub pl_header: PowerlinkHeader,
    // SoC frames do not carry a payload outside of the minimal padding/CRC.
}

impl SocFrame {
    /// Creates a new SoC frame.
    /// The NMT_Control field (Octets 4-5) typically holds the NMT Command ID for explicit commands.
    pub fn new(source_mac: [u8; 6], nmt_command_id: UNSIGNED16) -> Self {
        // SoC Destination MAC is always the specific SoC multicast address.
        let eth_header = EthernetHeader::new(
            crate::types::C_DLL_MULTICAST_SOC, 
            source_mac
        );
        
        // Octet 0: DLL_FrameType: ID 0x1 (Soc), Payload Length Code 0.
        let frame_type_and_payload_code: u8 = 0x10; 
        
        let pl_header = PowerlinkHeader {
            frame_type_and_payload_code,
            dll_identity: 0, 
            source_node_id: crate::types::C_ADR_MN_DEF_NODE_ID, // MN always sends SoC
            destination_node_id: 0, // Ignored in multicast frames
            nmt_control: nmt_command_id.to_be(), // NMT Command ID is mandatory
            frame_specific_data: 0, // Reserved
        };

        SocFrame { eth_header, pl_header }
    }
}

// --- Start of Asynchronous (SoA) ---

/// Requested Service IDs (DS 301, Appendix 3.4)
/// These values are encoded in the SoA's frame_specific_data field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RequestedServiceId {
    IdentRequest = 0x01, 
    StatusRequest = 0x02, 
    NmtCommand = 0x04,
    // ... others follow but these are mandatory for Phase 1/2 functionality
    Reserved = 0x00, 
}

/// Represents a complete SoA frame (MN multicast control message requesting an asynchronous response).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SoAFrame {
    pub eth_header: EthernetHeader,
    pub pl_header: PowerlinkHeader,
    // SoA frames do not carry a payload outside of the minimal padding/CRC.
}

impl SoAFrame {
    /// Creates a new SoA frame, requesting a specific service from a target Node ID.
    ///
    /// `target_node_id`: The CN Node ID being requested (0xFF for broadcast IdentRequest).
    /// `requested_service`: The service the MN is requesting (e.g., IdentRequest or StatusRequest).
    pub fn new(source_mac: [u8; 6], target_node_id: NodeId, requested_service: RequestedServiceId) -> Self {
        
        // SoA Destination MAC is always the specific SoA multicast address.
        let eth_header = EthernetHeader::new(
            crate::types::C_DLL_MULTICAST_SOA, 
            source_mac
        );

        // Octet 0: DLL_FrameType: ID 0x5 (SoA), Payload Length Code 0.
        let frame_type_and_payload_code: u8 = 0x50; 

        // Octet 6-9 (frame_specific_data) encodes the request:
        // Bits 31-24: RequestedServiceID
        // Bits 7-0: RequestedServiceTarget (target node ID)
        let requested_id_u32 = (requested_service as UNSIGNED32) << 24;
        let requested_target_u32 = target_node_id as UNSIGNED32;
        let frame_specific_data = (requested_id_u32 | requested_target_u32).to_be(); // Must be Big Endian (network order)

        let pl_header = PowerlinkHeader {
            frame_type_and_payload_code,
            dll_identity: 0, 
            source_node_id: crate::types::C_ADR_MN_DEF_NODE_ID, 
            destination_node_id: 0, // Ignored in multicast frames
            nmt_control: 0, 
            frame_specific_data,
        };

        SoAFrame { eth_header, pl_header }
    }
}