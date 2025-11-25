// src/common.rs
use crate::types::{UNSIGNED16, UNSIGNED32};

/// Represents a 64-bit NetTime value as defined by IEEE 1588.
/// Structure: Seconds (U32) + Nanoseconds (U32).
/// (EPSG DS 301, Section 6.1.6.7)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NetTime {
    pub seconds: UNSIGNED32,
    pub nanoseconds: UNSIGNED32,
}

impl NetTime {
    /// Creates a new NetTime instance.
    pub fn new(seconds: u32, nanoseconds: u32) -> Self {
        Self { seconds, nanoseconds }
    }

    /// Creates a NetTime from a microsecond timestamp (e.g., standard system clock).
    pub fn from_micros(micros: u64) -> Self {
        let seconds = (micros / 1_000_000) as u32;
        let nanoseconds = ((micros % 1_000_000) * 1_000) as u32;
        Self {
            seconds,
            nanoseconds,
        }
    }
    
    /// Returns the total nanoseconds (saturating at u64 max).
    pub fn as_nanos(&self) -> u64 {
        (self.seconds as u64)
            .saturating_mul(1_000_000_000)
            .saturating_add(self.nanoseconds as u64)
    }

    /// Adds a duration in nanoseconds to the NetTime.
    /// Handles rollover of nanoseconds into seconds.
    pub fn add_nanos(&mut self, nanos: u64) {
        let additional_seconds = (nanos / 1_000_000_000) as u32;
        let additional_nanos = (nanos % 1_000_000_000) as u32;

        self.seconds = self.seconds.wrapping_add(additional_seconds);
        self.nanoseconds += additional_nanos;

        if self.nanoseconds >= 1_000_000_000 {
            self.seconds = self.seconds.wrapping_add(1);
            self.nanoseconds -= 1_000_000_000;
        }
    }

    /// Calculates the signed difference between two NetTimes in nanoseconds.
    /// Returns `self - other`.
    ///
    /// Useful for calculating clock offset: `local_time.sub_net_time(received_soc_time)`.
    /// Returns `i64::MAX` or `i64::MIN` on overflow/underflow of the i64 range, 
    /// though typical synchronization offsets should fit well within 64 bits.
    pub fn sub_net_time(&self, other: NetTime) -> i64 {
        let sec_diff = self.seconds as i64 - other.seconds as i64;
        let nano_diff = self.nanoseconds as i64 - other.nanoseconds as i64;
        
        // sec_diff * 1e9 + nano_diff
        match sec_diff.checked_mul(1_000_000_000) {
            Some(s_ns) => s_ns.saturating_add(nano_diff),
            None => {
                if sec_diff > 0 {
                    i64::MAX
                } else {
                    i64::MIN
                }
            }
        }
    }
}

/// Represents a 64-bit RelativeTime value used in the SoC frame.
///
/// Defined as UNSIGNED64, transmitted in microseconds [us].
/// (EPSG DS 301, Section 4.6.1.1.2, Table 16)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RelativeTime(pub u64);

impl RelativeTime {
    /// Creates a new RelativeTime from microseconds.
    pub fn new(micros: u64) -> Self {
        Self(micros)
    }

    /// Adds a duration in microseconds to the relative time.
    pub fn add_micros(&mut self, micros: u64) {
        self.0 = self.0.wrapping_add(micros);
    }
}

/// Represents the TIME_OF_DAY data type.
/// (EPSG DS 301, Section 6.1.6.4)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TimeOfDay {
    /// Milliseconds after midnight.
    pub ms: UNSIGNED32,
    /// Days since January 1, 1984.
    pub days: UNSIGNED16,
}

/// Represents the TIME_DIFFERENCE data type.
/// (EPSG DS 301, Section 6.1.6.5)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TimeDifference {
    /// Milliseconds portion of the difference.
    pub ms: UNSIGNED32,
    /// Days portion of the difference.
    pub days: UNSIGNED16,
}