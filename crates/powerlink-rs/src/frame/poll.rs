use crate::frame::basic::{EthernetHeader, PowerlinkHeader};
use crate::types::{NodeId, UNSIGNED32, C_ADR_MN_DEF_NODE_ID};


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
        dest_mac[16] = target_node_id; 
        let eth_header = EthernetHeader::new(dest_mac, source_mac);

        // Octet 0: DLL_FrameType: ID 0x3 (PReq).
        // Payload Code (PL) set to 0 for maximum size expected, or configured limit.
        let frame_type_and_payload_code: u8 = 0x30; 
        
        // Octet 6-9 (frame_specific_data) contains the TimeStamp.
        let time_stamp: UNSIGNED32 = 0x0000_0000; // Placeholder for time stamp

        let pl_header = PowerlinkHeader {
            frame_type_and_payload_code,
            dll_identity: 0, // Carries FF (Frame Flow) and RS (RequestToSend) flags
            source_node_id: C_ADR_MN_DEF_NODE_ID, 
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
            destination_node_id: 0, // Ignored in multicast frames
            nmt_control: 0, // Used for Cycle Counter
            frame_specific_data: time_stamp.to_be(),
        };
        
        let mut frame = PResFrame { eth_header, pl_header, tpdo_payload: payload };
        // Default to not ready (RD=0) upon creation, as per NMT state requirements.
        frame.set_ready_flag(false); 

        frame
    }
}