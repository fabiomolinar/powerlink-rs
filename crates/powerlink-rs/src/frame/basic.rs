use crate::types::{MessageType, NodeId, C_DLL_ETHERTYPE_EPL, UNSIGNED16, UNSIGNED32};
use alloc::vec::Vec;
use core::fmt;

// --- Constants and Sizes ---

pub const MAC_ADDRESS_SIZE: usize = 6;
pub const ETHERNET_HEADER_SIZE: usize = 14;
/// The size of the core POWERLINK protocol header (10 bytes, offsets 0 to 9).
pub const EPL_HEADER_SIZE: usize = 10;
/// Total size of mandatory Ethernet II frame header plus POWERLINK header.
pub const TOTAL_HEADER_SIZE: usize = ETHERNET_HEADER_SIZE + EPL_HEADER_SIZE;

// --- MacAddress ---

/// A 6-byte IEEE 802 MAC address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MacAddress(pub [u8; MAC_ADDRESS_SIZE]);

impl MacAddress {
    /// Creates a new `MacAddress` from a 6-byte array.
    pub const fn new(bytes: [u8; MAC_ADDRESS_SIZE]) -> Self {
        MacAddress(bytes)
    }

    /// Checks if the address is a multicast address.
    pub fn is_multicast(&self) -> bool {
        // The first bit of the first octet is 1 for multicast addresses.
        (self.0[0] & 0x01) != 0
    }

    /// Checks if the address is the broadcast address (FF:FF:FF:FF:FF:FF).
    pub fn is_broadcast(&self) -> bool {
        self.0 == [0xFF; 6]
    }
}

impl fmt::Display for MacAddress {
    /// Formats the MAC address as "XX:XX:XX:XX:XX:XX".
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            self.0[0], self.0[1], self.0[2], self.0[3], self.0[4], self.0[5]
        )
    }
}

// --- NetTime and RelativeTime ---

/// Represents a 64 bits NetTime value as defined by IEEE 1588.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetTime {
    pub seconds: UNSIGNED32, // Seconds part (upper 32 bits)
    pub nanoseconds: UNSIGNED32, // Nanoseconds part (lower 32 bits)
}

/// Represents a 64 bits RelativeTime value as defined by IEEE 1588.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelativeTime {
    pub seconds: UNSIGNED32, // Seconds part (upper 32 bits)
    pub nanoseconds: UNSIGNED32, // Nanoseconds part (lower 32 bits)
}

// --- Ethernet Header ---

/// Represents a standard 14-byte Ethernet Header (Layer 2).
/// Structure: Destination MAC (6), Source MAC (6), EtherType (2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EthernetHeader {
    // UPDATED: Use the new MacAddress type
    pub destination_mac: MacAddress,
    pub source_mac: MacAddress,
    // Stored network byte order (big-endian) but often processed as host order in Rust.
    pub ether_type: UNSIGNED16, 
    // CRC is not included here as it's typically handled by other layers or hardware.
}

impl EthernetHeader {
    /// Checks if the EtherType matches the required POWERLINK value (0x88AB).
    pub fn is_powerlink(&self) -> bool {
        self.ether_type.to_be() == C_DLL_ETHERTYPE_EPL
    }

    /// Creates a new header destined for a specific unicast or multicast MAC address.
    // UPDATED: The constructor now takes MacAddress types
    pub fn new(dest: MacAddress, src: MacAddress) -> Self {
        Self {
            destination_mac: dest,
            source_mac: src,
            // EtherType must be stored as Big Endian (network byte order)
            ether_type: C_DLL_ETHERTYPE_EPL.to_be(),
        }
    }
}

// --- MessageTypeOctet ---

/// Struct representing the message type octet.
pub struct MessageTypeOctet {
    pub message_type: MessageType,
}

// --- Powerlink Header ---

/// The core 10-byte POWERLINK frame header (DS 301, Table 41).
#[repr(packed)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PowerlinkHeader {
    // Octet 0: Partially reserved and MessageType.
    // Bit 7: Reserved (0)
    // Bits 6 - 0: MessageType (4 bits) + Payload Length Code (4 bits)
    pub message_type: MessageTypeOctet,
    
    // Octet 1: Destination.
    pub destination: NodeId,
    
    // Octet 2: Source.
    pub source: NodeId,
    
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
    
    // Methods for serialization and deserialization to be added here in subsequent commits/phases.
}

// --- Unit Tests ---

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ethernet_header_is_powerlink() {
        // UPDATED: Use MacAddress::new() for clarity
        let mut header = EthernetHeader::new(MacAddress::new([0; 6]), MacAddress::new([0; 6]));
        assert!(header.is_powerlink());

        header.ether_type = 0x0800; // IP packet
        assert!(!header.is_powerlink());
    }
    
    #[test]
    fn test_ethernet_header_new() {
        // UPDATED: Use the new type for construction
        let dest_mac = MacAddress::new([0x01, 0x11, 0x1E, 0x00, 0x00, 0x01]);
        let src_mac = MacAddress::new([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
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