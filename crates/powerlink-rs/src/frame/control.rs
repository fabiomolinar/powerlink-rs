use crate::frame::basic::{
    EthernetHeader, PowerlinkHeader, MacAddress
};
use crate::types::{
    NodeId, UNSIGNED16, UNSIGNED32, C_ADR_MN_DEF_NODE_ID, 
    C_DLL_MULTICAST_SOA, C_DLL_MULTICAST_SOC
};

// --- Start of Cycle (SoC) ---

/// Represents a complete SoC frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SocFrame {
    pub eth_header: EthernetHeader,
    pub pl_header: PowerlinkHeader,
    octet3: u8,
    octet4: u8,
    octet5: u8,
    pub net_time: NetTime,
    pub relative_time: RelativeTime,
    octet22_45: [u8; 24], // Reserved/Padding
}

impl SocFrame {
    /// Creates a new SoC frame.
    /// The NMT_Control field (Octets 4-5) typically holds the NMT Command ID for explicit commands.
    pub fn new(
        dest: MacAddress, source: MacAddress
    ) -> Self {
        // SoC Destination MAC is always the specific SoC multicast address.
        let eth_header = EthernetHeader::new(
            MacAddress(C_DLL_MULTICAST_SOC), 
            MacAddress(source_mac)
        );
        
        // Octet 0: DLL_FrameType: ID 0x1 (Soc), Payload Length Code 0.
        let frame_type_and_payload_code: u8 = 0x10; 
        
        let pl_header = PowerlinkHeader {
            frame_type_and_payload_code,
            dll_identity: 0, 
            source_node_id: NodeId(C_ADR_MN_DEF_NODE_ID), // MN always sends SoC
            destination_node_id: NodeId(0), // Ignored in multicast frames
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
            MacAddress(C_DLL_MULTICAST_SOA), 
            MacAddress(source_mac)
        );

        // Octet 0: DLL_FrameType: ID 0x5 (SoA), Payload Length Code 0.
        let frame_type_and_payload_code: u8 = 0x50; 

        // Octet 6-9 (frame_specific_data) encodes the request:
        // Bits 31-24: RequestedServiceID
        // Bits 7-0: RequestedServiceTarget (target node ID)
        let requested_id_u32 = (requested_service as UNSIGNED32) << 24;
        let requested_target_u32 = target_node_id.0 as UNSIGNED32;
        let frame_specific_data = (requested_id_u32 | requested_target_u32).to_be(); // Must be Big Endian (network order)

        let pl_header = PowerlinkHeader {
            frame_type_and_payload_code,
            dll_identity: 0, 
            source_node_id: NodeId(C_ADR_MN_DEF_NODE_ID), 
            destination_node_id: NodeId(0), // Ignored in multicast frames
            nmt_control: 0, 
            frame_specific_data,
        };

        SoAFrame { eth_header, pl_header }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{C_DLL_MULTICAST_SOC, C_DLL_MULTICAST_SOA};
    

    #[test]
    fn test_socframe_new_constructor() {
        let source_mac = [0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC];
        let nmt_command: u16 = 0x0020; // Example: NMTResetNode
        let frame = SocFrame::new(source_mac, nmt_command);

        // Check Ethernet header
        assert_eq!(frame.eth_header.destination_mac.0, C_DLL_MULTICAST_SOC);
        assert_eq!(frame.eth_header.source_mac.0, source_mac);

        // Check POWERLINK header
        assert_eq!(frame.pl_header.get_message_type(), Some(crate::types::MessageType::Soc));
        assert_eq!(frame.pl_header.get_payload_code(), 0);
        assert_eq!(frame.pl_header.source_node_id, NodeId(C_ADR_MN_DEF_NODE_ID));
        assert_eq!(frame.pl_header.destination_node_id, NodeId(0));
        
        // Copy packed fields to local variables before asserting.
        let nmt_control = frame.pl_header.nmt_control;
        let frame_specific_data = frame.pl_header.frame_specific_data;
        assert_eq!(nmt_control, nmt_command.to_be());
        assert_eq!(frame_specific_data, 0);
    }
    
    #[test]
    fn test_soaframe_new_constructor_builds_correct_header() {
        let source_mac = [0xFE, 0xDC, 0xBA, 0x98, 0x76, 0x54];
        let target_node = NodeId(42);
        let service = RequestedServiceId::StatusRequest; // ID 0x02
        
        let frame = SoAFrame::new(source_mac, target_node, service);

        // Check Ethernet header
        assert_eq!(frame.eth_header.destination_mac.0, C_DLL_MULTICAST_SOA);
        assert_eq!(frame.eth_header.source_mac.0, source_mac);

        // Check POWERLINK header
        assert_eq!(frame.pl_header.get_message_type(), Some(crate::types::MessageType::SoA));
        assert_eq!(frame.pl_header.source_node_id, NodeId(C_ADR_MN_DEF_NODE_ID));

        // The most important check: verify the frame_specific_data encoding.
        // RequestedServiceID (0x02) in bits 31-24, TargetNodeID (42 = 0x2A) in bits 7-0.
        // Expected value in host order: 0x0200002A
        let expected_data = (0x0200002A as u32).to_be();
        
        // FIX: Copy the packed field to a local variable before asserting.
        let frame_specific_data = frame.pl_header.frame_specific_data;
        assert_eq!(frame_specific_data, expected_data);
    }
}