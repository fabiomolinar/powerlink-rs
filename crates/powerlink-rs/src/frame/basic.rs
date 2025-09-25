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

// --- Priority Flag ---

// A tuple struct is the most concise way to implement the Newtype Pattern
pub struct RSFlag(u8); 

impl RSFlag {
    /// Creates a new RSFlag value, ensuring the input is within 0-7.
    /// If a higher value is provided, it will be clamped to 7.
    pub fn new(value: u8) -> Self {
        let clamped_value = value.min(7);
        RSFlag(clamped_value)       
    }

    /// Provides safe, read-only access to the underlying u8 value.
    pub fn get(&self) -> u8 {
        self.0
    }

    /// Add 1 to the current RSFlag value, clamping at 7.
    pub fn inc(&mut self) {
        if self.0 < 7 {
            self.0 += 1;
        }
    }

    /// Subtract 1 from the current RSFlag value, clamping at 0.
    pub fn dec(&mut self) {
        if self.0 > 0 {
            self.0 -= 1;
        }
    }

    /// Sets the RSFlag to a new value, clamping at 7 if necessary.
    pub fn set(&mut self, value: u8) {
        self.0 = value.min(7);
    }
}

// --- Priority Flag ---

#[allow(non_camel_case_types)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    pub message_type_octet: MessageTypeOctet,
    
    // Octet 1: Destination.
    pub destination: NodeId,
    
    // Octet 2: Source.
    pub source: NodeId,
    
}

impl PowerlinkHeader {
    /// Creates a new PowerlinkHeader with specified parameters.
    pub fn new(
        message_type: MessageType,
        destination: NodeId,
        source: NodeId,
    ) -> Self {
        let message_type_octet = MessageTypeOctet { message_type };
        
        PowerlinkHeader {
            message_type_octet,
            destination,
            source,
        }
    }
    /// Extracts the MessageType (Frame ID, bits 7-4 of Octet 0).
    pub fn get_message_type(&self) -> MessageType {
        return self.message_type_octet.message_type;
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
            message_type_octet: MessageTypeOctet { message_type: MessageType::SoC },
            destination: NodeId(1),
            source: NodeId(2),
        };
        assert_eq!(header.get_message_type(), MessageType::SoC);

        header.message_type_octet = MessageTypeOctet { message_type: MessageType::PReq };
        assert_eq!(header.get_message_type(), MessageType::PReq);

        header.message_type_octet = MessageTypeOctet { message_type: MessageType::PRes };
        assert_eq!(header.get_message_type(), MessageType::PRes);

        header.message_type_octet = MessageTypeOctet { message_type: MessageType::SoA };
        assert_eq!(header.get_message_type(), MessageType::SoA);

        header.message_type_octet = MessageTypeOctet { message_type: MessageType::ASnd };
        assert_eq!(header.get_message_type(), MessageType::ASnd);
    }
}