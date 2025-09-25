//+ NEW FILE
use crate::types::{UNSIGNED32};

/// Represents a 64 bits NetTime value as defined by IEEE 1588.
/// (EPSG DS 301, Section 6.1.6.7)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetTime {
    pub seconds: UNSIGNED32,
    pub nanoseconds: UNSIGNED32,
}

/// Represents a 64 bits RelativeTime value.
/// (EPSG DS 301, Section 4.6.1.1.2)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelativeTime {
    pub seconds: UNSIGNED32,
    pub nanoseconds: UNSIGNED32,
}