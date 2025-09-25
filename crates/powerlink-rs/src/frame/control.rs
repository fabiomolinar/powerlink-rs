use crate::frame::basic::{
    EthernetHeader, PowerlinkHeader, MacAddress, NetTime, RelativeTime
};
use crate::types::{
    NodeId, UNSIGNED16, UNSIGNED32, C_ADR_MN_DEF_NODE_ID, 
    C_DLL_MULTICAST_SOA, C_DLL_MULTICAST_SOC, MessageType,
    C_ADR_BROADCAST_NODE_ID, EPLVersion
};
use crate::nmt::{self, NMTState};

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
        source_mac: MacAddress, mc_flag: bool, ps_flag: bool,
        net_time: NetTime, relative_time: RelativeTime,
    ) -> Self {
        // SoC Destination MAC is always the specific SoC multicast address.
        let eth_header = EthernetHeader::new(
            MacAddress(C_DLL_MULTICAST_SOC), 
            source_mac
        );                
        let pl_header = PowerlinkHeader::new(
            MessageType::SoC, // MessageType ID for SoC
            NodeId(C_ADR_BROADCAST_NODE_ID), // Destination Node ID is ignored in multicast frames
            NodeId(C_ADR_MN_DEF_NODE_ID), // Source Node ID (MN)
        );
        let mut octet4 : u8 = 0x00;
        if ps_flag { octet4 |= 0b01000000; }
        if mc_flag { octet4 |= 0b10000000; }
        
        SocFrame {
            eth_header,
            pl_header,
            octet3: 0x00, // Reserved
            octet4,
            octet5: 0x00, // Reserved
            net_time,
            relative_time,
            octet22_45: [0x00; 24],
        }
    }
    /// Retrieve the MC flag from octet4.
    pub fn get_mc_flag(&self) -> bool {
        (self.octet4 & 0b10000000) != 0
    }
    /// Retrieve the PS flag from octet4.
    pub fn get_ps_flag(&self) -> bool {
        (self.octet4 & 0b01000000) != 0
    }
}

// --- Start of Asynchronous (SoA) ---

/// Requested Service IDs (DS 301, Appendix 3.4)
/// These values are encoded in the SoA's frame_specific_data field.
#[allow(non_camel_case_types)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RequestedServiceId {
    NO_SERVICE = 0x00,
    IDENT_REQUEST = 0x01, 
    STATUS_REQUEST = 0x02, 
    NMT_REQUEST_INVITE = 0x03,         
    UNSPECIFIED_INVITE = 0xFF, 
}

/// Represents a complete SoA frame (MN multicast control message requesting an asynchronous response).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SoAFrame {
    pub eth_header: EthernetHeader,
    pub pl_header: PowerlinkHeader,
    pub nmt_state: NMTState,
    octet4: u8,
    octet5: u8,
    pub req_service_id: RequestedServiceId,
    pub target_node_id: NodeId,
    pub epl_version: EPLVersion,
    octet9_45: [u8; 37], // Reserved/Padding
}

impl SoAFrame {
    /// Creates a new SoA frame, requesting a specific service from a target Node ID.
    pub fn new(
        source_mac: MacAddress, nmt_state: NMTState, ea_flag: bool,
        er_flag: bool, requested_service: RequestedServiceId, target_node_id: NodeId,
        epl_version: EPLVersion,
    ) -> Self {        
        // SoA Destination MAC is always the specific SoA multicast address.
        let eth_header = EthernetHeader::new(
            MacAddress(C_DLL_MULTICAST_SOA), 
            source_mac
        );
        let pl_header = PowerlinkHeader::new(
            MessageType::SoA, // MessageType ID for SoC
            NodeId(C_ADR_BROADCAST_NODE_ID), // Destination Node ID is ignored in multicast frames
            NodeId(C_ADR_MN_DEF_NODE_ID), // Source Node ID (MN)
        );
        let mut octet4 : u8 = 0x00;
        if er_flag { octet4 |= 0b00000010; }
        if ea_flag { octet4 |= 0b00000100; }
        SoAFrame { 
            eth_header,
            pl_header,
            nmt_state,
            octet4,
            octet5: 0x00, // Reserved
            req_service_id: requested_service,
            target_node_id,
            epl_version,
            octet9_45: [0x00; 37],
         }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{C_DLL_MULTICAST_SOC, C_DLL_MULTICAST_SOA};
    

    #[test]
    fn test_socframe_new_constructor() {
        let source_mac = MacAddress([0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC]);
        let dummy_time = NetTime{seconds: 0xABCD, nanoseconds: 0xABCD};
        let dummy_rel_time = RelativeTime{seconds: 0xABCD, nanoseconds: 0xABCD};
        let frame = SocFrame::new(
            source_mac, true, false, 
            dummy_time, dummy_rel_time
        );

        // Check Ethernet header
        assert_eq!(frame.eth_header.destination_mac.0, C_DLL_MULTICAST_SOC);
        assert_eq!(frame.eth_header.source_mac, source_mac);

        // Check POWERLINK header
        assert_eq!(frame.pl_header.get_message_type(), MessageType::SoC);
        assert_eq!(frame.pl_header.source, NodeId(C_ADR_MN_DEF_NODE_ID));
        assert_eq!(frame.pl_header.destination, NodeId(C_ADR_BROADCAST_NODE_ID));
    }
    
    #[test]
    fn test_soaframe_new_constructor_builds_correct_header() {
        let source_mac = MacAddress([0xFE, 0xDC, 0xBA, 0x98, 0x76, 0x54]);
        let target_node = NodeId(42);
        let service = RequestedServiceId::STATUS_REQUEST; // ID 0x02
        
        let frame = SoAFrame::new(
            source_mac, NMTState{}, true, false,
            service, target_node, EPLVersion(1)
        );

        // Check Ethernet header
        assert_eq!(frame.eth_header.destination_mac.0, C_DLL_MULTICAST_SOA);
        assert_eq!(frame.eth_header.source_mac, source_mac);

        // Check POWERLINK header
        assert_eq!(frame.pl_header.get_message_type(), MessageType::SoA);
        assert_eq!(frame.pl_header.source, NodeId(C_ADR_MN_DEF_NODE_ID));
    }
}