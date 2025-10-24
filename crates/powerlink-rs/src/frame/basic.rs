use crate::types::{C_DLL_ETHERTYPE_EPL, UNSIGNED16};
use core::fmt;

// --- Constants and Sizes ---

pub const MAC_ADDRESS_SIZE: usize = 6;
pub const ETHERNET_HEADER_SIZE: usize = 14;

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

impl From<[u8; 6]> for MacAddress {
    /// Allows for ergonomic conversion from a raw byte array.
    /// E.g., `let my_array = [0u8; 6]; let mac: MacAddress = my_array.into();`
    fn from(bytes: [u8; 6]) -> Self {
        MacAddress(bytes)
    }
}

// --- Ethernet Header ---

/// Represents a standard 14-byte Ethernet Header (Layer 2).
/// Structure: Destination MAC (6), Source MAC (6), EtherType (2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EthernetHeader {
    pub destination_mac: MacAddress,
    pub source_mac: MacAddress,
    pub ether_type: UNSIGNED16,
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
            ether_type: C_DLL_ETHERTYPE_EPL.to_be(),
        }
    }
}

// --- Unit Tests ---

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ethernet_header_is_powerlink() {
        let mut header = EthernetHeader::new(MacAddress::new([0; 6]), MacAddress::new([0; 6]));
        assert!(header.is_powerlink());

        header.ether_type = 0x0800; // IP packet
        assert!(!header.is_powerlink());
    }

    #[test]
    fn test_ethernet_header_new() {
        let dest_mac = MacAddress::new([0x01, 0x11, 0x1E, 0x00, 0x00, 0x01]);
        let src_mac = MacAddress::new([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
        let header = EthernetHeader::new(dest_mac, src_mac);

        assert_eq!(header.destination_mac, dest_mac);
        assert_eq!(header.source_mac, src_mac);
        assert_eq!(header.ether_type, C_DLL_ETHERTYPE_EPL.to_be());
    }
}
