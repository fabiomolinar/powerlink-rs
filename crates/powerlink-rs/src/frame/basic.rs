use crate::types::{MessageType, NodeId, C_DLL_ETHERTYPE_EPL, UNSIGNED16, UNSIGNED32};
use alloc::vec::Vec;


pub const MAC_ADDRESS_SIZE: usize = 6;
pub const ETHERNET_HEADER_SIZE: usize = 14;
/// The size of the core POWERLINK protocol header (10 bytes, offsets 0 to 9).
pub const EPL_HEADER_SIZE: usize = 10;
/// Total size of mandatory Ethernet II frame header plus POWERLINK header.
pub const TOTAL_HEADER_SIZE: usize = ETHERNET_HEADER_SIZE + EPL_HEADER_SIZE;

/// Represents a standard 14-byte Ethernet Header (Layer 2).
/// Structure: Destination MAC (6), Source MAC (6), EtherType (2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EthernetHeader {
    pub destination_mac: [u8; MAC_ADDRESS_SIZE],
    pub source_mac: [u8; MAC_ADDRESS_SIZE],
    // Stored network byte order (big-endian) but often processed as host order in Rust.
    pub ether_type: UNSIGNED16, 
}

impl EthernetHeader {
    /// Checks if the EtherType matches the required POWERLINK value (0x88AB).
    pub fn is_powerlink(&self) -> bool {
        // Assume `ether_type` is stored in the correct endianness or check both if ambiguity exists.
        // For Rust network programming, this usually means checking against the BE representation.
        self.ether_type.to_be() == C_DLL_ETHERTYPE_EPL
    }

    /// Creates a new header destined for a specific unicast or multicast MAC address.
    pub fn new(dest: [u8; MAC_ADDRESS_SIZE], src: [u8; MAC_ADDRESS_SIZE]) -> Self {
        Self {
            destination_mac: dest,
            source_mac: src,
            // EtherType must be stored as Big Endian (network byte order)
            ether_type: C_DLL_ETHERTYPE_EPL.to_be(),
        }
    }
}

/// The core 10-byte POWERLINK frame header (DS 301, Table 41).
#[repr(packed)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PowerlinkHeader {
    // Octet 0: DLL_FrameType.
    // Bits 7-4: Message Type ID (e.g., 0x1 for SoC, 0x3 for PReq)
    // Bits 3-0: Payload Length (PL) indicator (used for negotiation or indicating size)
    pub frame_type_and_payload_code: u8,
    
    // Octet 1: DLL_Identity.
    // Carries flags like MS (Multiplexed Slot) and potentially PR (Priority) in PRes/ASnd.
    pub dll_identity: u8,
    
    // Octet 2: DLL_SourceNodeID.
    pub source_node_id: NodeId,
    
    // Octet 3: DLL_DestinationNodeID.
    pub destination_node_id: NodeId,

    // Octet 4-5: NMT_Control (Cycle Counter in PReq/PRes, NMT Command ID in SoC/SoA).
    pub nmt_control: UNSIGNED16, 
    
    // Octet 6-9: Frame specific data (e.g., Time Stamps in Isochronous frames, Service IDs in Asynchronous).
    pub frame_specific_data: UNSIGNED32,
}

impl PowerlinkHeader {
    /// Extracts the MessageType (Frame ID, bits 7-4 of Octet 0).
    pub fn get_message_type(&self) -> Option<MessageType> {
        let id = self.frame_type_and_payload_code >> 4;
        match id {
            0x1 => Some(MessageType::Soc),
            0x3 => Some(MessageType::PReq),
            0x4 => Some(MessageType::PRes),
            0x5 => Some(MessageType::SoA),
            0x6 => Some(MessageType::ASnd),
            _ => None,
        }
    }
    
    /// Extracts the Payload Length Code (PL, bits 3-0 of Octet 0).
    pub fn get_payload_code(&self) -> u8 {
        self.frame_type_and_payload_code & 0x0F
    }
    
    // Methods for serialization and deserialization would be added here in subsequent commits/phases.
}

/// Represents a complete DLL frame, combining Ethernet framing and POWERLINK protocol header/payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PowerlinkFrame {
    pub ethernet_header: EthernetHeader,
    pub powerlink_header: PowerlinkHeader,
    /// Raw payload bytes (PDO or SDO/NMT data), padded to minimum frame size if necessary.
    pub payload: Vec<u8>, 
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ethernet_header_is_powerlink() {
        let mut header = EthernetHeader::new([0; 6], [0; 6]);
        assert!(header.is_powerlink());

        header.ether_type = 0x0800; // IP packet
        assert!(!header.is_powerlink());
    }
    
    #[test]
    fn test_ethernet_header_new() {
        let dest_mac = [0x01, 0x11, 0x1E, 0x00, 0x00, 0x01];
        let src_mac = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];
        let header = EthernetHeader::new(dest_mac, src_mac);
        
        assert_eq!(header.destination_mac, dest_mac);
        assert_eq!(header.source_mac, src_mac);
        assert_eq!(header.ether_type, C_DLL_ETHERTYPE_EPL.to_be());
    }

    #[test]
    fn test_powerlink_header_get_message_type() {
        let mut header = PowerlinkHeader {
            frame_type_and_payload_code: 0x1A, // SoC with payload code 10
            dll_identity: 0,
            source_node_id: NodeId(0),
            destination_node_id: NodeId(0),
            nmt_control: 0,
            frame_specific_data: 0,
        };

        assert_eq!(header.get_message_type(), Some(MessageType::Soc));

        header.frame_type_and_payload_code = 0x3F; // PReq
        assert_eq!(header.get_message_type(), Some(MessageType::PReq));
        
        header.frame_type_and_payload_code = 0x40; // PRes
        assert_eq!(header.get_message_type(), Some(MessageType::PRes));

        header.frame_type_and_payload_code = 0x51; // SoA
        assert_eq!(header.get_message_type(), Some(MessageType::SoA));
        
        header.frame_type_and_payload_code = 0x62; // ASnd
        assert_eq!(header.get_message_type(), Some(MessageType::ASnd));
        
        header.frame_type_and_payload_code = 0x20; // Reserved message type
        assert_eq!(header.get_message_type(), None);
    }
    
    #[test]
    fn test_powerlink_header_get_payload_code() {
        let header = PowerlinkHeader {
            frame_type_and_payload_code: 0x1A, // Message type 1, payload code 10 (0xA)
            dll_identity: 0,
            source_node_id: NodeId(0),
            destination_node_id: NodeId(0),
            nmt_control: 0,
            frame_specific_data: 0,
        };
        assert_eq!(header.get_payload_code(), 10);
        
        let header_zero = PowerlinkHeader {
            frame_type_and_payload_code: 0x30, // Message type 3, payload code 0
            ..header
        };
        assert_eq!(header_zero.get_payload_code(), 0);
        
        let header_max = PowerlinkHeader {
            frame_type_and_payload_code: 0xFF, // Message type 15, payload code 15
            ..header
        };
        assert_eq!(header_max.get_payload_code(), 15);
    }
}