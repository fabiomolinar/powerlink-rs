use crate::frame::basic::{EthernetHeader, PowerlinkHeader};
use crate::types::{NodeId, UNSIGNED32, C_ADR_MN_DEF_NODE_ID};
use alloc::vec::Vec;


// --- Poll Request (PReq) ---

/// Represents a Poll Request frame (MN unicast frame to CN).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PReqFrame {
    pub eth_header: EthernetHeader,
    pub pl_header: PowerlinkHeader,
    /// The payload contains Receive PDO (RPDO) data for the addressed CN.
    pub rpdo_payload: Vec<u8>,
}

impl PReqFrame {
    /// Creates a PReq frame destined for a specific Controlled Node.
    ///
    /// NOTE: Destination MAC calculation based on Node ID is omitted here,
    /// using a placeholder based on the Node ID itself for demo.
    pub fn new(source_mac: [u8; 6], target_node_id: NodeId, payload: Vec<u8>) -> Self {
        
        // PReq frames are unicast. Destination MAC derived from Node ID (e.g., 00-00-00-00-00-CN_ID)
        // This is a simplification; actual derivation depends on configuration or network rules.
        let mut dest_mac: [u8; 6] = [0x00; 6];
        dest_mac[5] = target_node_id.try_into().unwrap(); 
        let eth_header = EthernetHeader::new(dest_mac, source_mac);

        // Octet 0: DLL_FrameType: ID 0x3 (PReq).
        // Payload Code (PL) set to 0 for maximum size expected, or configured limit.
        let frame_type_and_payload_code: u8 = 0x30; 
        
        // Octet 6-9 (frame_specific_data) contains the TimeStamp.
        let time_stamp: UNSIGNED32 = 0x0000_0000; // Placeholder for time stamp

        let pl_header = PowerlinkHeader {
            frame_type_and_payload_code,
            dll_identity: 0, // Carries FF (Frame Flow) and RS (RequestToSend) flags
            source_node_id: NodeId(C_ADR_MN_DEF_NODE_ID), 
            destination_node_id: target_node_id, 
            nmt_control: 0, // Used for Cycle Counter
            frame_specific_data: time_stamp.to_be(), 
        };
        
        PReqFrame { eth_header, pl_header, rpdo_payload: payload }
    }
}

// --- Poll Response (PRes) ---

/// Represents a Poll Response frame (CN multicast frame to all nodes/MN).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PResFrame {
    pub eth_header: EthernetHeader,
    pub pl_header: PowerlinkHeader,
    /// The payload contains Transmit PDO (TPDO) data for the network.
    pub tpdo_payload: Vec<u8>,
}

impl PResFrame {
    /// Sets or clears the Ready (RD) flag, which indicates if the payload data is valid.
    /// The RD flag is located in the DLL_Identity octet.
    /// In NMT_CS_READY_TO_OPERATE, the RD flag shall be reset (0).
    pub fn set_ready_flag(&mut self, ready: bool) {
        if ready {
            // Set Bit 7 of DLL_Identity (0x80)
            self.pl_header.dll_identity |= 0x80; 
        } else {
            // Clear Bit 7 of DLL_Identity
            self.pl_header.dll_identity &= !0x80;
        }
    }

    /// Creates a PRes frame originating from a Controlled Node (or MN, PResMN).
    pub fn new(source_mac: [u8; 6], source_node_id: NodeId, payload: Vec<u8>) -> Self {
        
        // PRes frames use the specific PRes multicast address.
        let eth_header = EthernetHeader::new(
            crate::types::C_DLL_MULTICAST_PRES, 
            source_mac
        );
        
        // Octet 0: DLL_FrameType: ID 0x4 (PRes).
        let frame_type_and_payload_code: u8 = 0x40; 
        
        // Octet 6-9 (frame_specific_data) contains the TimeStamp.
        let time_stamp: UNSIGNED32 = 0x0000_0000; 

        let pl_header = PowerlinkHeader {
            frame_type_and_payload_code,
            // DLL_Identity (Octet 1) carries NMT State and Flags (RD, PR).
            dll_identity: 0, 
            source_node_id, 
            destination_node_id: NodeId(0), // Ignored in multicast frames
            nmt_control: 0, // Used for Cycle Counter
            frame_specific_data: time_stamp.to_be(),
        };
        
        let mut frame = PResFrame { eth_header, pl_header, tpdo_payload: payload };
        // Default to not ready (RD=0) upon creation, as per NMT state requirements.
        frame.set_ready_flag(false); 

        frame
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{MessageType, C_DLL_MULTICAST_PRES};
    use alloc::vec;


    #[test]
    fn test_preqframe_new_constructor() {
        let source_mac = [0xAA; 6];
        let target_node = NodeId(55);
        let payload = vec![0x01, 0x02, 0x03];
        let frame = PReqFrame::new(source_mac, target_node, payload.clone());
        
        // Simplified dest MAC check
        let expected_dest_mac = [0x00, 0x00, 0x00, 0x00, 0x00, 55];
        assert_eq!(frame.eth_header.destination_mac, expected_dest_mac);
        assert_eq!(frame.eth_header.source_mac, source_mac);
        
        assert_eq!(frame.pl_header.get_message_type(), Some(MessageType::PReq));
        assert_eq!(frame.pl_header.source_node_id, NodeId(C_ADR_MN_DEF_NODE_ID));
        assert_eq!(frame.pl_header.destination_node_id, target_node);
        
        assert_eq!(frame.rpdo_payload, payload);
    }
    
    #[test]
    fn test_presframe_new_and_ready_flag() {
        let source_mac = [0xBB; 6];
        let source_node = NodeId(10);
        let payload = vec![0xA, 0xB, 0xC, 0xD];
        let mut frame = PResFrame::new(source_mac, source_node, payload.clone());
        
        // Check initial state from new()
        assert_eq!(frame.eth_header.destination_mac, C_DLL_MULTICAST_PRES);
        assert_eq!(frame.pl_header.get_message_type(), Some(MessageType::PRes));
        assert_eq!(frame.pl_header.source_node_id, source_node);
        assert_eq!(frame.tpdo_payload, payload);
        
        // Test ready flag logic
        // Should be false (0) by default from new()
        assert_eq!(frame.pl_header.dll_identity & 0x80, 0);

        // Set ready to true
        frame.set_ready_flag(true);
        assert_eq!(frame.pl_header.dll_identity & 0x80, 0x80);
        
        // Set ready to false again
        frame.set_ready_flag(false);
        assert_eq!(frame.pl_header.dll_identity & 0x80, 0);
    }
}