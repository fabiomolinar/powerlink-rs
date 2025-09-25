#![allow(non_camel_case_types)]

use crate::frame::basic::{EthernetHeader, MacAddress};
use crate::nmt::states::{NMTState};
use crate::types::{
    NodeId, C_ADR_MN_DEF_NODE_ID, C_DLL_MULTICAST_PRES, 
    MessageType, C_ADR_BROADCAST_NODE_ID
};
use crate::pdo::{self, PDOVersion};
use alloc::vec::Vec;

// --- Request to Send (RS) Flag ---

/// A newtype wrapper for the 3-bit RS (Request to Send) flag.
/// (EPSG DS 301, Section 4.2.4.1.2.2)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RSFlag(u8); 

impl RSFlag {
    /// Creates a new RSFlag, clamping the value to the valid 0-7 range.
    pub fn new(value: u8) -> Self {
        RSFlag(value.min(7))       
    }

    /// Provides read-only access to the underlying u8 value.
    pub fn get(&self) -> u8 {
        self.0
    }
}

// --- Priority (PR) Flag ---

/// An enum for the 3-bit PR (Priority) flag.
/// (EPSG DS 301, Appendix 3.2)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PRFlag{
    PRIO_NMT_REQUEST = 0b111,
    High3 = 0b110,
    High2 = 0b101,
    High1 = 0b100,
    PRIO_GENERIC_REQUEST = 0b011,
    Low3 = 0b010,
    Low2 = 0b001,
    Low1 = 0b000,
}

// --- Poll Request (PReq) ---

/// Represents a Poll Request frame (MN unicast frame to CN).
/// (EPSG DS 301, Section 4.6.1.1.3)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PReqFrame {
    pub eth_header: EthernetHeader,
    pub message_type: MessageType,
    pub destination: NodeId,
    pub source: NodeId,
    pub flags: PReqFlags,
    pub pdo_version: PDOVersion,
    pub payload_size: u16,
    pub payload: Vec<u8>,
}

/// Flags specific to the PReq frame.
/// (EPSG DS 301, Table 18)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PReqFlags {
    pub ms: bool, // Multiplexed Slot
    pub ea: bool, // Exception Acknowledge
    pub rd: bool, // Ready
}

impl PReqFrame {
    /// Creates a PReq frame destined for a specific Controlled Node.
    pub fn new(
        source_mac: MacAddress,
        dest_mac: MacAddress,
        target_node_id: NodeId,
        flags: PReqFlags,
        pdo_version: PDOVersion,
        payload: Vec<u8>
    ) -> Self {
        let eth_header = EthernetHeader::new(dest_mac, source_mac);
        let payload_size = payload.len() as u16;
        
        PReqFrame { 
            eth_header,
            message_type: MessageType::PReq,
            destination: target_node_id,
            source: NodeId(C_ADR_MN_DEF_NODE_ID),
            flags,
            pdo_version,
            payload_size,
            payload,
         }
    }
}

// --- Poll Response (PRes) ---

/// Represents a Poll Response frame (CN multicast frame).
/// (EPSG DS 301, Section 4.6.1.1.4)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PResFrame {
    pub eth_header: EthernetHeader,
    pub message_type: MessageType,
    pub destination: NodeId,
    pub source: NodeId,
    pub nmt_state: NMTState,
    pub flags: PResFlags,
    pub pdo_version : PDOVersion,
    pub payload_size : u16,
    pub payload: Vec<u8>,
}

/// Flags specific to the PRes frame.
/// (EPSG DS 301, Table 20)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PResFlags {
    pub ms: bool, // Multiplexed Slot
    pub en: bool, // Exception New
    pub rd: bool, // Ready
    pub pr: PRFlag,
    pub rs: RSFlag,
}

impl Default for PRFlag {
    fn default() -> Self {
        PRFlag::Low1
    }
}

impl PResFrame {
    /// Creates a new PRes frame.
    pub fn new(
        source_mac: MacAddress,
        source_node_id: NodeId,
        nmt_state: NMTState,
        flags: PResFlags,
        pdo_version: PDOVersion,
        payload: Vec<u8>
    ) -> Self {
        let eth_header = EthernetHeader::new(
            MacAddress(C_DLL_MULTICAST_PRES),
            source_mac
        );                
        let payload_size = payload.len() as u16;
        
        PResFrame { 
            eth_header,
            message_type: MessageType::PRes,
            destination: NodeId(C_ADR_BROADCAST_NODE_ID),
            source: source_node_id,
            nmt_state,
            flags,
            pdo_version,
            payload_size,
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
        let flags = PReqFlags { ms: true, ea: false, rd: true };
        let frame = PReqFrame::new(
            source_mac, 
            MacAddress([0x00, 0x00, 0x00, 0x00, 0x00, 55]),
            target_node,
            flags,
            PDOVersion(1),
            payload.clone()
        );
        
        let expected_dest_mac = [0x00, 0x00, 0x00, 0x00, 0x00, 55];
        assert_eq!(frame.eth_header.destination_mac.0, expected_dest_mac);
        assert_eq!(frame.eth_header.source_mac, source_mac);
        assert_eq!(frame.message_type, MessageType::PReq);
        assert_eq!(frame.source, NodeId(C_ADR_MN_DEF_NODE_ID));
        assert_eq!(frame.destination, target_node);
        assert_eq!(frame.payload, payload);
        assert!(frame.flags.rd);
    }
    
    #[test]
    fn test_presframe_new_constructor() {
        let source_mac = MacAddress([0xBB; 6]);
        let source_node = NodeId(10);
        let payload = vec![0xA, 0xB, 0xC, 0xD];
        let flags = PResFlags {
            ms: true,
            en: true,
            rd: false,
            pr: PRFlag::High1,
            rs: RSFlag::new(5),
        };
        let frame = PResFrame::new(
            source_mac,
            source_node,
            NMTState::NMT_CS_NOT_ACTIVE,
            flags,
            PDOVersion(1),
            payload.clone()
        );
        
        assert_eq!(frame.eth_header.destination_mac.0, C_DLL_MULTICAST_PRES);
        assert_eq!(frame.message_type, MessageType::PRes);
        assert_eq!(frame.source, source_node);
        assert_eq!(frame.payload, payload);
        assert!(!frame.flags.rd);
        assert_eq!(frame.flags.rs.get(), 5);
    }
}