// crates/powerlink-rs/src/frame/error/status_response.rs
use crate::PowerlinkError;
use crate::common::NetTime;

// The StaticErrorBitField struct has been moved to crates/powerlink-rs/src/frame/control/status_response.rs

/// Represents the Mode field within an Error Entry's EntryType.
/// (Reference: EPSG DS 301, Table 96)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ErrorEntryMode {
    Terminator = 0,
    ErrorActive = 1,
    ErrorCleared = 2,
    EventOccurred = 3,
}

impl TryFrom<u8> for ErrorEntryMode {
    type Error = PowerlinkError;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Terminator),
            1 => Ok(Self::ErrorActive),
            2 => Ok(Self::ErrorCleared),
            3 => Ok(Self::EventOccurred),
            _ => Err(PowerlinkError::InvalidEnumValue),
        }
    }
}

/// Represents the 16-bit EntryType field of an Error Entry.
/// (Reference: EPSG DS 301, Table 96)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntryType {
    /// Indicates if this is a Status Entry or a History Entry.
    pub is_status_entry: bool,
    /// Indicates if the entry should also be placed in the Emergency Queue.
    pub send_to_queue: bool,
    /// The mode of the error/event (e.g., active, cleared).
    pub mode: ErrorEntryMode,
    /// The profile that defines the error code (e.g., POWERLINK, vendor-specific).
    pub profile: u16,
}

impl EntryType {
    pub fn from_u16(value: u16) -> Result<Self, PowerlinkError> {
        Ok(Self {
            is_status_entry: (value & (1 << 15)) != 0,
            send_to_queue: (value & (1 << 14)) != 0,
            mode: ErrorEntryMode::try_from(((value >> 12) & 0b11) as u8)?,
            profile: value & 0x0FFF,
        })
    }
}

/// Represents a single 20-byte Error/Event History Entry.
/// (Reference: EPSG DS 301, Table 94)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ErrorEntry {
    pub entry_type: EntryType,
    pub error_code: u16,
    pub timestamp: NetTime,
    pub additional_information: u64,
}

impl ErrorEntry {
    pub fn deserialize(buffer: &[u8]) -> Result<Self, PowerlinkError> {
        if buffer.len() < 20 {
            return Err(PowerlinkError::BufferTooShort);
        }
        let entry_type_val = u16::from_le_bytes(buffer[0..2].try_into()?);
        let error_code = u16::from_le_bytes(buffer[2..4].try_into()?);
        let seconds = u32::from_le_bytes(buffer[4..8].try_into()?);
        let nanoseconds = u32::from_le_bytes(buffer[8..12].try_into()?);
        let additional_information = u64::from_le_bytes(buffer[12..20].try_into()?);

        Ok(Self {
            entry_type: EntryType::from_u16(entry_type_val)?,
            error_code,
            timestamp: NetTime {
                seconds,
                nanoseconds,
            },
            additional_information,
        })
    }
}