use core::convert::TryFrom;
use core::fmt;

// --- Primitive Types (Based on DS 301 Section 6.1.4) ---

/// Alias for BOOLEAN (8-bit unsigned integer, 0 or 1).
pub type BOOLEAN = u8;
/// Alias for INTEGER8 (8-bit signed integer).
pub type INTEGER8 = i8;
/// Alias for INTEGER16 (16-bit signed integer).
pub type INTEGER16 = i16;
/// Alias for INTEGER24 (32-bit signed integer, though only 24 bits used).
pub type INTEGER24 = i32;
/// Alias for INTEGER32 (32-bit signed integer).
pub type INTEGER32 = i32;
/// Alias for UNSIGNED8 (8-bit unsigned integer).
pub type UNSIGNED8 = u8;
/// Alias for UNSIGNED16 (16-bit unsigned integer).
pub type UNSIGNED16 = u16;
/// Alias for UNSIGNED24 (32-bit unsigned integer, though only 24 bits used).
pub type UNSIGNED24 = u32;
/// Alias for UNSIGNED32 (32-bit unsigned integer).
pub type UNSIGNED32 = u32;
/// Alias for REAL32 (32-bit floating point).
pub type REAL32 = f32;
/// Alias for REAL64 (64-bit floating point).
pub type REAL64 = f64;
/// Alias for a 6-byte MAC Address.
pub type MAC_ADDRESS = [u8; 6];
/// Alias for a 4-byte IP Address.
pub type IP_ADDRESS = [u8; 4];


/// Represents a POWERLINK Node ID, wrapping a `u8` to ensure type safety.
///
/// Valid Node IDs are in the range 1-240, with special values for broadcast (255)
/// and routers/management (253-254). This newtype pattern prevents accidental
/// use of invalid `u8` values where a `NodeId` is required.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(pub u8);

/// Represents the POWERLINK Version.
/// (EPSG DS 301, Table 112) [cite: 112]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EPLVersion(pub u8);

// --- Protocol Constants (Appendix 3) ---

/// Ethernet EtherType for POWERLINK frames: 0x88AB[cite: 828].
pub const C_DLL_ETHERTYPE_EPL: u16 = 0x88AB;

/// Maximum size of PReq and PRes payload data (1490 Bytes)[cite: 857, 866].
pub const C_DLL_ISOCHR_MAX_PAYL: u16 = 1490;

/// Maximum asynchronous payload in bytes (1500 Bytes)[cite: 1620].
pub const C_DLL_MAX_ASYNC_MTU: usize = 1500;

/// POWERLINK default Node ID of the Managing Node (240).
pub const C_ADR_MN_DEF_NODE_ID: u8 = 240;

/// Maximum Node ID available for regular Controlled Nodes (239)[cite: 819].
pub const C_ADR_MAX_CN_NODE_ID: u8 = 239;

/// POWERLINK Node ID for diagnostic device (253)[cite: 825].
pub const C_ADR_DIAG_DEF_NODE_ID: u8 = 253;

/// POWERLINK Node ID for router (254)[cite: 825].
pub const C_ADR_RT1_DEF_NODE_ID: u8 = 254;

/// POWERLINK Node ID for broadcast messages (255)[cite: 825].
pub const C_ADR_BROADCAST_NODE_ID: u8 = 255;

/// POWERLINK PRes multicast MAC address: 01-11-1E-00-00-02[cite: 815].
pub const C_DLL_MULTICAST_PRES: [u8; 6] = [0x01, 0x11, 0x1E, 0x00, 0x00, 0x02];

/// POWERLINK SoA multicast MAC address: 01-11-1E-00-00-03[cite: 815].
pub const C_DLL_MULTICAST_SOA: [u8; 6] = [0x01, 0x11, 0x1E, 0x00, 0x00, 0x03];

/// POWERLINK SoC multicast MAC address: 01-11-1E-00-00-01[cite: 815].
pub const C_DLL_MULTICAST_SOC: [u8; 6] = [0x01, 0x11, 0x1E, 0x00, 0x00, 0x01];

// --- Core Protocol Identifiers ---

/// Defines the mandatory POWERLINK Message Type IDs.
/// (EPSG DS 301, Appendix 3.1) 
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MessageType {
    SoC = 0x01,
    PReq = 0x03,
    PRes = 0x04,
    SoA = 0x05,
    ASnd = 0x06,
}

/// Error type for invalid Node ID creation.
#[derive(Debug, PartialEq, Eq)]
pub enum NodeIdError {
    /// Node ID is outside the valid range.
    InvalidRange(u8),
}

impl fmt::Display for NodeIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NodeIdError::InvalidRange(value) => write!(f, "Invalid NodeId value: {}. Valid range is 1-240 or 253-255.", value),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for NodeIdError {}

impl TryFrom<u8> for NodeId {
    type Error = NodeIdError;

    /// Creates a `NodeId` from a `u8`, returning an error if the value is not valid.
    ///
    /// Valid IDs are 1-240, 253, 254, and 255[cite: 825].
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1..=C_ADR_MN_DEF_NODE_ID => Ok(NodeId(value)),
            C_ADR_DIAG_DEF_NODE_ID | C_ADR_RT1_DEF_NODE_ID | C_ADR_BROADCAST_NODE_ID => Ok(NodeId(value)),
            _ => Err(NodeIdError::InvalidRange(value)),
        }
    }
}

impl From<NodeId> for u8 {
    /// Converts a `NodeId` back into its underlying `u8` representation.
    fn from(node_id: NodeId) -> Self {
        node_id.0
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nodeid_valid_ranges() {
        assert_eq!(NodeId::try_from(1), Ok(NodeId(1)));
        assert_eq!(NodeId::try_from(239), Ok(NodeId(239)));
        assert_eq!(NodeId::try_from(240), Ok(NodeId(C_ADR_MN_DEF_NODE_ID)));
        assert_eq!(NodeId::try_from(253), Ok(NodeId(C_ADR_DIAG_DEF_NODE_ID)));
        assert_eq!(NodeId::try_from(254), Ok(NodeId(C_ADR_RT1_DEF_NODE_ID)));
        assert_eq!(NodeId::try_from(255), Ok(NodeId(C_ADR_BROADCAST_NODE_ID)));
    }

    #[test]
    fn test_nodeid_invalid_range() {
        assert_eq!(NodeId::try_from(0), Err(NodeIdError::InvalidRange(0)));
        assert_eq!(NodeId::try_from(241), Err(NodeIdError::InvalidRange(241)));
        assert_eq!(NodeId::try_from(252), Err(NodeIdError::InvalidRange(252)));
    }
}