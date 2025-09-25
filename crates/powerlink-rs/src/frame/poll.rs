use crate::frame::basic::{
    EthernetHeader, PowerlinkHeader, MacAddress, RSFlag,
    PRFlag
};
use crate::nmt::{self, NMTState};
use crate::types::{
    NodeId, UNSIGNED32, C_ADR_MN_DEF_NODE_ID, 
    C_DLL_MULTICAST_PRES, MessageType,
    C_ADR_BROADCAST_NODE_ID
};
use crate::pdo::{self, PDOVersion};
use alloc::vec::Vec;


// --- Poll Request (PReq) ---

/// Represents a Poll Request frame (MN unicast frame to CN).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PReqFrame {
    pub eth_header: EthernetHeader,
    pub pl_header: PowerlinkHeader,
    octet3 : u8,
    octet4 : u8,
    octet5 : u8,
    pub pdo_version : PDOVersion,
    octet7 : u8,
    pub payload_size : u16,
    pub payload: Vec<u8>,
}

impl PReqFrame {
    /// Creates a PReq frame destined for a specific Controlled Node.
    ///
    /// NOTE: Destination MAC calculation based on Node ID is omitted here,
    /// using a placeholder based on the Node ID itself for demo.
    pub fn new(
        source_mac: MacAddress, dest_mac: MacAddress, target_node_id: NodeId,
        ms_flag: bool, ea_flag: bool, rd_flag : bool, pdo_version: PDOVersion,
        payload_size: u16, payload: Vec<u8>
    ) -> Self {
        let eth_header = EthernetHeader::new(
            dest_mac, 
            source_mac
        );                
        let pl_header = PowerlinkHeader::new(
            MessageType::PReq, // MessageType ID for SoC
            target_node_id,
            NodeId(C_ADR_MN_DEF_NODE_ID), // Source Node ID (MN)
        );
        let mut octet4 : u8 = 0x00;
        if ms_flag { octet4 |= 0b00100000; }
        if ea_flag { octet4 |= 0b00000100; }
        if rd_flag { octet4 |= 0b00000001; }
        
        PReqFrame { 
            eth_header,
            pl_header,
            octet3: 0x00, // Reserved
            octet4,
            octet5: 0x00, // Reserved
            pdo_version,
            octet7: 0x00, // Reserved
            payload_size,
            payload,
         }
    }
}

// --- Poll Response (PRes) ---

/// Represents a Poll Response frame (CN unicast or multicast frame to MN).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PResFrame {
    pub eth_header: EthernetHeader,
    pub pl_header: PowerlinkHeader,
    pub nmt_state: NMTState,
    octet4 : u8,
    octet5 : u8,
    pub pdo_version : PDOVersion,
    octet7 : u8,
    pub payload_size : u16,
    pub payload: Vec<u8>,
}

impl PResFrame {
    /// Creates a PReq frame destined for a specific Controlled Node.
    ///
    /// NOTE: Destination MAC calculation based on Node ID is omitted here,
    /// using a placeholder based on the Node ID itself for demo.
    pub fn new(
        source_mac: MacAddress, nmt_state: NMTState, ms_flag: bool, 
        en_flag: bool, rd_flag : bool, pr_flag : PRFlag, 
        rs_flag: RSFlag, pdo_version: PDOVersion, payload_size: u16, 
        payload: Vec<u8>
    ) -> Self {
        // PRes shall be transmitted using the multicast MAC address
        let eth_header = EthernetHeader::new(
            MacAddress(C_DLL_MULTICAST_PRES),
            source_mac
        );                
        let pl_header = PowerlinkHeader::new(
            MessageType::PRes, // MessageType ID for SoC
            NodeId(C_ADR_BROADCAST_NODE_ID), // Destination Node ID is ignored in multicast frames
            NodeId(C_ADR_MN_DEF_NODE_ID), // Source Node ID (MN)
        );
        let mut octet4 : u8 = 0x00;
        if ms_flag { octet4 |= 0b00100000; }
        if en_flag { octet4 |= 0b00010000; }
        if rd_flag { octet4 |= 0b00000001; }
        // Combine PRFlag and RSFlag into octet5.
        let mut octet5 : u8 = 0x00;
        octet5 |= (pr_flag as u8 & 0b00000111) << 3; // PRFlag in bits 5-3
        octet5 |= rs_flag.get() & 0b00000111;
        
        PResFrame { 
            eth_header,
            pl_header,
            nmt_state, // Default state
            octet4,
            octet5,
            pdo_version,
            octet7: 0x00, // Reserved
            payload_size,
            payload,
        }
    }
}


// --- Asynchronous Send (ASnd) ---

/// Requested Service IDs (DS 301, Appendix 3.4)
/// These values are encoded in the SoA's frame_specific_data field.
#[allow(non_camel_case_types)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ServiceId {
    IDENT_RESPONSE = 0x01,
    STATUS_RESPONSE = 0x02, 
    NMT_REQUEST = 0x03, 
    NMT_COMMAND = 0x04,         
    SDO = 0x05, 
}

/// Represents a complete ASnd frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ASndFrame {
    pub eth_header: EthernetHeader,
    pub pl_header: PowerlinkHeader,
    pub service_id: ServiceId,
    pub payload: Vec<u8>,
}

impl ASndFrame {
    /// Creates a new ASnd frame.
    pub fn new(
        source_mac: MacAddress, dest_mac: MacAddress, target_node_id: NodeId,
        source_node_id: NodeId, service_id: ServiceId, payload: Vec<u8>
    ) -> Self {
        let eth_header = EthernetHeader::new(
            dest_mac, 
            source_mac
        );                
        let pl_header = PowerlinkHeader::new(
            MessageType::ASnd, // MessageType ID for SoC
            target_node_id,
            source_node_id,
        );
        
        ASndFrame { 
            eth_header,
            pl_header,
            service_id,
            payload,
         }
    }
}




#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{MessageType, C_DLL_MULTICAST_PRES};
    use alloc::vec;


    #[test]
    fn test_preqframe_new_constructor() {
        let source_mac = MacAddress([0xAA; 6]);
        let target_node = NodeId(55);
        let payload = vec![0x01, 0x02, 0x03];
        let frame = PReqFrame::new(
            source_mac, 
            MacAddress([0x00, 0x00, 0x00, 0x00, 0x00, 55]), // Simplified dest MAC for demo
            target_node,
            true,  // ms_flag
            false, // ea_flag
            true,  // rd_flag
            PDOVersion(1),
            payload.len() as u16,
            payload.clone()
        );
        
        // Simplified dest MAC check
        let expected_dest_mac = [0x00, 0x00, 0x00, 0x00, 0x00, 55];
        assert_eq!(frame.eth_header.destination_mac.0, expected_dest_mac);
        assert_eq!(frame.eth_header.source_mac, source_mac);
        
        assert_eq!(frame.pl_header.get_message_type(), MessageType::PReq);
        assert_eq!(frame.pl_header.source, NodeId(C_ADR_MN_DEF_NODE_ID));
        assert_eq!(frame.pl_header.destination, target_node);
        
        assert_eq!(frame.payload, payload);
    }
    
    #[test]
    fn test_presframe_new_and_ready_flag() {
        let source_mac = [0xBB; 6];
        let source_node = NodeId(10);
        let payload = vec![0xA, 0xB, 0xC, 0xD];
        let frame = PResFrame::new(
            MacAddress(source_mac),
            NMTState{},
            true,  // ms_flag
            true,  // en_flag
            false, // rd_flag
            PRFlag::High1,
            RSFlag::new(5),
            PDOVersion(1),
            payload.len() as u16,
            payload.clone()
        );
        
        // Check initial state from new()
        assert_eq!(frame.eth_header.destination_mac.0, C_DLL_MULTICAST_PRES);
        assert_eq!(frame.pl_header.get_message_type(), MessageType::PRes);
        assert_eq!(frame.pl_header.source, source_node);
        assert_eq!(frame.payload, payload);
        
    }
}