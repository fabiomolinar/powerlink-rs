/// Represents the NMT Feature Flags from Object 0x1F82 as a type-safe bitmask.
/// (Reference: EPSG DS 301, Section 7.2.1.1.6, Table 111)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FeatureFlags(pub u32);

impl FeatureFlags {
    // --- Flag Constants ---
    pub const ISOCHRONOUS: Self = Self(1 << 0);
    pub const SDO_UDP: Self = Self(1 << 1);
    pub const SDO_ASND: Self = Self(1 << 2);
    pub const SDO_PDO: Self = Self(1 << 3);
    pub const NMT_INFO_SERVICES: Self = Self(1 << 4);
    pub const EXTENDED_NMT_CMDS: Self = Self(1 << 5);
    pub const DYNAMIC_PDO_MAPPING: Self = Self(1 << 6);
    pub const NMT_SERVICE_UDP: Self = Self(1 << 7);
    pub const CONFIG_MANAGER: Self = Self(1 << 8);
    pub const MULTIPLEXED_ACCESS: Self = Self(1 << 9);
    pub const NODE_ID_BY_SW: Self = Self(1 << 10);
    pub const MN_BASIC_ETHERNET: Self = Self(1 << 11);
    pub const ROUTING_TYPE_1: Self = Self(1 << 12);
    pub const ROUTING_TYPE_2: Self = Self(1 << 13);
    pub const SDO_RW_ALL_BY_INDEX: Self = Self(1 << 14);
    pub const SDO_RW_MULTIPLE_BY_INDEX: Self = Self(1 << 15);

    // --- Methods ---

    /// Creates a new `FeatureFlags` struct from a raw u32 value.
    pub fn from_bits_truncate(bits: u32) -> Self {
        Self(bits)
    }

    /// Checks if all of the specified flags are set.
    pub fn contains(&self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    /// Returns an empty set of flags.
    pub fn empty() -> Self {
        Self(0)
    }

    /// Inserts the specified flags.
    pub fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }

    /// Removes the specified flags.
    pub fn remove(&mut self, other: Self) {
        self.0 &= !other.0;
    }
}