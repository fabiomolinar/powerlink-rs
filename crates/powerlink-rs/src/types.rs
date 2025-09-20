use core::convert::TryFrom;
use core::fmt;

// --- Primitive Types (Based on DS 301 Section 6.1.4) ---
// These aliases ensure compatibility with object dictionary definitions (UNSIGNEDn)

/// Alias for UNSIGNED8 (8-bit unsigned integer)
pub type UNSIGNED8 = u8;
/// Alias for UNSIGNED16 (16-bit unsigned integer)
pub type UNSIGNED16 = u16;
/// Alias for UNSIGNED32 (32-bit unsigned integer)
pub type UNSIGNED32 = u32;

/// Represents a POWERLINK Node ID, wrapping a `u8` to ensure type safety.
///
/// Valid Node IDs are in the range 1-240, with special values for broadcast (255)
/// and asynchronous management (254). This newtype pattern prevents accidental
/// use of invalid `u8` values where a `NodeId` is required.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(pub u8);

// --- Protocol Constants (Appendix 3) ---

/// Ethernet EtherType for POWERLINK frames: 0x88AB
pub const C_DLL_ETHERTYPE_EPL: u16 = 0x88AB;

/// Maximum size of PReq and PRes payload data (1490 Byte)
pub const C_DLL_ISOCHR_MAX_PAYL: usize = 1490;

/// Maximum asynchronous payload in bytes including all headers (exclusive the Ethernet header) (1500 Byte)
pub const C_DLL_MAX_ASYNC_MTU: usize = 1500;

/// POWERLINK default Node ID of the Managing Node (240 or F0h)
pub const C_ADR_MN_DEF_NODE_ID: u8 = 240;

/// Maximum Node ID available for regular Controlled Nodes (239)
pub const C_ADR_MAX_CN_NODE_ID: u8 = 239;

/// POWERLINK Node ID for asynchronous management messages (254 or FEh)
pub const C_ADR_ASYNC_MGMT_NODE_ID: u8 = 254;

/// POWERLINK Node ID for broadcast messages (255 or FFh)
pub const C_ADR_BROADCAST_NODE_ID: u8 = 255;

/// POWERLINK PRes multicast MAC address: 01-11-1E-00-00-02
pub const C_DLL_MULTICAST_PRES: [u8; 6] = [0x01, 0x11, 0x1E, 0x00, 0x00, 0x02];

/// POWERLINK SoA multicast MAC address: 01-11-1E-00-00-03
pub const C_DLL_MULTICAST_SOA: [u8; 6] = [0x01, 0x11, 0x1E, 0x00, 0x00, 0x03];

/// POWERLINK SoC multicast MAC address: 01-11-1E-00-00-01
pub const C_DLL_MULTICAST_SOC: [u8; 6] = [0x01, 0x11, 0x1E, 0x00, 0x00, 0x01];


// --- Core Protocol Identifiers ---

/// Defines the mandatory POWERLINK Message Type IDs (DS 301, Appendix 3.1)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MessageType {
    // Isochronous frames
    Soc = 0x01, // Start of Cycle
    PReq = 0x03, // Poll Request
    PRes = 0x04, // Poll Response
    // Asynchronous frames
    SoA = 0x05, // Start of Asynchronous
    ASnd = 0x06, // Asynchronous Send
    // Other values exist (e.g., 0x02 is reserved, 0x08 is NmtRequest/ServiceRequest), using placeholder for foundation
    Reserved = 0x00,
}

/// Error type for invalid Node ID creation.
#[derive(Debug, PartialEq, Eq)]
pub enum NodeIdError {
    /// Node ID is outside the valid range (1-240, 254, 255).
    InvalidRange(u8),
}

impl fmt::Display for NodeIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NodeIdError::InvalidRange(value) => write!(f, "Invalid NodeId value: {}. Valid range is 1-240, 254, or 255.", value),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for NodeIdError {}

impl TryFrom<u8> for NodeId { // Now this is `impl TryFrom<u8> for NodeId` and not `... for u8`
    type Error = NodeIdError;

    /// Creates a `NodeId` from a `u8`, returning an error if the value is not a valid
    /// POWERLINK node identifier.
    ///
    /// Valid IDs are 1-239 (CN), 240 (MN), 254 (Async Mgmt), and 255 (Broadcast).
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            // Regular CN and MN range
            1..=C_ADR_MN_DEF_NODE_ID => Ok(NodeId(value)),
            // Special valid IDs
            C_ADR_ASYNC_MGMT_NODE_ID | C_ADR_BROADCAST_NODE_ID => Ok(NodeId(value)),
            // All other values are invalid
            _ => Err(NodeIdError::InvalidRange(value)),
        }
    }
}

impl From<NodeId> for u8 {
    /// Converts a `NodeId` back into its underlying `u8` representation.
    /// This conversion is infallible.
    fn from(node_id: NodeId) -> Self {
        node_id.0
    }
}