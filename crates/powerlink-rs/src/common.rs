use crate::types::{UNSIGNED16, UNSIGNED32};

/// Represents a 64-bit NetTime value as defined by IEEE 1588.
/// (EPSG DS 301, Section 6.1.6.7)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetTime {
    pub seconds: UNSIGNED32,
    pub nanoseconds: UNSIGNED32,
}

/// Represents a 64-bit RelativeTime value used in the SoC frame.
/// (EPSG DS 301, Section 4.6.1.1.2)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelativeTime {
    pub seconds: UNSIGNED32,
    pub nanoseconds: UNSIGNED32,
}

/// Represents the TIME_OF_DAY data type.
/// (EPSG DS 301, Section 6.1.6.4)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeOfDay {
    /// Milliseconds after midnight.
    pub ms: UNSIGNED32,
    /// Days since January 1, 1984.
    pub days: UNSIGNED16,
}

/// Represents the TIME_DIFFERENCE data type.
/// (EPSG DS 301, Section 6.1.6.5)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeDifference {
    /// Milliseconds portion of the difference.
    pub ms: UNSIGNED32,
    /// Days portion of the difference.
    pub days: UNSIGNED16,
}