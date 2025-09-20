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