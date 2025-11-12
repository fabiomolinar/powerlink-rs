//! Defines the structure and codec for the StatusResponse service payload.

use crate::frame::error::{ErrorEntry, ErrorEntryMode};
use crate::frame::poll::{PRFlag, RSFlag};
use crate::hal::PowerlinkError;
use crate::nmt::states::NmtState;
use crate::od::{ObjectDictionary, constants};
use alloc::vec::Vec;
use core::convert::TryInto;
use log::warn;

pub const STATIC_ERROR_BIT_FIELD_SIZE: usize = 8;
const STATUS_PAYLOAD_HEADER_SIZE: usize = 14;
const ERROR_ENTRY_SIZE: usize = 20;

/// Represents the Static Error Bit Field from a StatusResponse frame.
/// This struct was moved from `frame/error/status_response.rs`.
/// (Reference: EPSG DS 301, Section 6.5.8.1)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StaticErrorBitField {
    /// Corresponds to the content of ERR_ErrorRegister_U8 (0x1001).
    pub error_register: u8,
    /// Device profile or vendor-specific error bits.
    pub specific_errors: [u8; 7],
}

impl StaticErrorBitField {
    /// Creates a new `StaticErrorBitField` by reading from the Object Dictionary.
    pub fn new(od: &ObjectDictionary) -> Self {
        Self {
            error_register: od.read_u8(constants::IDX_NMT_ERROR_REGISTER_U8, 0).unwrap_or(0),
            // TODO: Populate specific_errors from OD if defined
            specific_errors: [0; 7],
        }
    }

    /// Serializes the `StaticErrorBitField` into a buffer.
    pub fn serialize(&self, buffer: &mut [u8]) -> Result<usize, PowerlinkError> {
        if buffer.len() < STATIC_ERROR_BIT_FIELD_SIZE {
            return Err(PowerlinkError::BufferTooShort);
        }
        buffer[0] = self.error_register;
        buffer[1..8].copy_from_slice(&self.specific_errors);
        Ok(STATIC_ERROR_BIT_FIELD_SIZE)
    }

    /// Deserializes a `StaticErrorBitField` from a buffer.
    pub fn deserialize(buffer: &[u8]) -> Result<Self, PowerlinkError> {
        if buffer.len() < STATIC_ERROR_BIT_FIELD_SIZE {
            return Err(PowerlinkError::BufferTooShort);
        }
        Ok(Self {
            error_register: buffer[0],
            specific_errors: buffer[1..8].try_into()?,
        })
    }
}

/// Represents the payload of an ASnd(StatusResponse) frame.
///
/// This structure contains all fields defined in the NMT Service Slot
/// for a StatusResponse.
/// (Reference: EPSG DS 301, Section 7.3.3.3.1, Table 136)
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StatusResponsePayload {
    // Octet 0: Flags
    pub en_flag: bool,
    pub ec_flag: bool,
    // Octet 1: Flags
    pub pr: PRFlag,
    pub rs: RSFlag,
    // Octet 2: NMTState
    pub nmt_state: NmtState,
    // Octets 6-13
    pub static_error_bit_field: StaticErrorBitField,
    // Octets 14..
    pub error_entries: Vec<ErrorEntry>,
}

impl StatusResponsePayload {
    /// Creates a new `StatusResponsePayload` with the given data.
    pub fn new(
        en_flag: bool,
        ec_flag: bool,
        pr: PRFlag,
        rs: RSFlag,
        nmt_state: NmtState,
        static_error_bit_field: StaticErrorBitField,
        error_entries: Vec<ErrorEntry>,
    ) -> Self {
        Self {
            en_flag,
            ec_flag,
            pr,
            rs,
            nmt_state,
            static_error_bit_field,
            error_entries,
        }
    }

    /// Serializes the `StatusResponsePayload` into the provided buffer.
    ///
    /// Returns the total number of bytes written.
    pub fn serialize(&self, buffer: &mut [u8]) -> Result<usize, PowerlinkError> {
        // Calculate required size: Header + Entries + Terminator
        let required_size =
            STATUS_PAYLOAD_HEADER_SIZE + (self.error_entries.len() * ERROR_ENTRY_SIZE) + ERROR_ENTRY_SIZE;

        if buffer.len() < required_size {
            return Err(PowerlinkError::BufferTooShort);
        }

        // Fill header area with zeros (for reserved fields)
        buffer[..STATUS_PAYLOAD_HEADER_SIZE].fill(0);

        // Octet 0: EN/EC Flags
        buffer[0] = (if self.en_flag { 1 << 5 } else { 0 })
            | (if self.ec_flag { 1 << 4 } else { 0 });
        // Octet 1: PR/RS Flags
        buffer[1] = (self.pr as u8) << 3 | self.rs.get();
        // Octet 2: NMTState
        buffer[2] = self.nmt_state as u8;
        // Octets 3-5 are reserved (already 0)
        // Octets 6-13: StaticErrorBitField
        self.static_error_bit_field
            .serialize(&mut buffer[6..14])?;

        // Octets 14..: Error Entries
        let mut offset = STATUS_PAYLOAD_HEADER_SIZE;
        let mut entry_buffer = [0u8; ERROR_ENTRY_SIZE];
        for entry in &self.error_entries {
            // Manually serialize the ErrorEntry (as it's defined in another module)
            let entry_type_val = (entry.entry_type.profile & 0x0FFF)
                | ((entry.entry_type.mode as u16) << 12)
                | (if entry.entry_type.is_status_entry {
                    1 << 15
                } else {
                    0
                })
                | (if entry.entry_type.send_to_queue {
                    1 << 14
                } else {
                    0
                });

            entry_buffer[0..2].copy_from_slice(&entry_type_val.to_le_bytes());
            entry_buffer[2..4].copy_from_slice(&entry.error_code.to_le_bytes());
            entry_buffer[4..8].copy_from_slice(&entry.timestamp.seconds.to_le_bytes());
            entry_buffer[8..12].copy_from_slice(&entry.timestamp.nanoseconds.to_le_bytes());
            entry_buffer[12..20].copy_from_slice(&entry.additional_information.to_le_bytes());

            buffer[offset..offset + ERROR_ENTRY_SIZE].copy_from_slice(&entry_buffer);
            offset += ERROR_ENTRY_SIZE;
        }

        // Add the terminator entry (Mode=0)
        entry_buffer.fill(0);
        buffer[offset..offset + ERROR_ENTRY_SIZE].copy_from_slice(&entry_buffer);
        offset += ERROR_ENTRY_SIZE;

        Ok(offset) // Return total bytes written
    }

    /// Deserializes a `StatusResponsePayload` from a byte slice.
    pub fn deserialize(buffer: &[u8]) -> Result<Self, PowerlinkError> {
        if buffer.len() < STATUS_PAYLOAD_HEADER_SIZE {
            warn!(
                "StatusResponse payload too short. Expected min {}, got {}",
                STATUS_PAYLOAD_HEADER_SIZE,
                buffer.len()
            );
            return Err(PowerlinkError::BufferTooShort);
        }

        // Octet 0: Flags
        let octet0 = buffer[0];
        let en_flag = (octet0 & (1 << 5)) != 0;
        let ec_flag = (octet0 & (1 << 4)) != 0;

        // Octet 1: Flags
        let octet1 = buffer[1];
        let pr = PRFlag::try_from(octet1 >> 3)?;
        let rs = RSFlag::new(octet1 & 0b111);

        // Octet 2: NMTState
        let nmt_state = NmtState::try_from(buffer[2])?;

        // Octets 6-13: StaticErrorBitField
        let static_error_bit_field = StaticErrorBitField::deserialize(&buffer[6..14])?;

        // Octets 14..: Error Entries
        let mut error_entries = Vec::new();
        let mut offset = STATUS_PAYLOAD_HEADER_SIZE;
        while buffer.len() >= offset + ERROR_ENTRY_SIZE {
            let entry_slice = &buffer[offset..offset + ERROR_ENTRY_SIZE];
            let entry = ErrorEntry::deserialize(entry_slice)?;

            // The list is terminated by an entry with Mode = Terminator
            if entry.entry_type.mode == ErrorEntryMode::Terminator {
                break;
            }
            error_entries.push(entry);
            offset += ERROR_ENTRY_SIZE;
        }

        Ok(Self {
            en_flag,
            ec_flag,
            pr,
            rs,
            nmt_state,
            static_error_bit_field,
            error_entries,
        })
    }
}