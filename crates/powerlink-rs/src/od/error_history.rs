// crates/powerlink-rs/src/od/error_history.rs
use crate::frame::error::ErrorEntry;
use crate::od::{Object, ObjectDictionary, ObjectValue};
use log::error;

const IDX_ERR_HISTORY: u16 = 0x1003;
const MAX_HISTORY_ENTRIES: usize = 254;

/// Writes an error entry to the Error History (0x1003) in the Object Dictionary.
///
/// According to EPSG DS 301, 6.5.10.2:
/// "Sub-index 0 contains the number of actual errors/events...
/// Every new error/event is stored at sub-index 1, the older ones move down the list."
pub fn write_error_to_history(od: &mut ObjectDictionary, entry: &ErrorEntry) {
    // Access the entry directly. 
    // Note: This bypasses 'write_internal' checks, which is appropriate for this internal logic.
    if let Some(entry_obj) = od.entries.get_mut(&IDX_ERR_HISTORY) {
        if let Object::Array(ref mut values) = entry_obj.object {
            // Serialize the new entry to a byte vector (ObjectValue::Domain)
            let entry_bytes = entry.serialize();
            let object_value = ObjectValue::Domain(entry_bytes);

            // Insert at the beginning (Sub-Index 1 corresponds to index 0 in the Vec)
            values.insert(0, object_value);

            // Enforce maximum size
            if values.len() > MAX_HISTORY_ENTRIES {
                values.pop();
            }
        } else {
            error!("Error History (0x1003) is not an ARRAY.");
        }
    } else {
        // This is expected if the user hasn't configured 0x1003 in the OD, 
        // which is valid for a minimal CN. We just don't log.
    }
}

impl ErrorEntry {
    /// Serializes the ErrorEntry to a 20-byte array (Little Endian).
    pub fn serialize(&self) -> alloc::vec::Vec<u8> {
        let mut buf = alloc::vec![0u8; 20];
        
        let entry_type_val = (self.entry_type.profile & 0x0FFF)
            | ((self.entry_type.mode as u16) << 12)
            | (if self.entry_type.is_status_entry { 1 << 15 } else { 0 })
            | (if self.entry_type.send_to_queue { 1 << 14 } else { 0 });

        buf[0..2].copy_from_slice(&entry_type_val.to_le_bytes());
        buf[2..4].copy_from_slice(&self.error_code.to_le_bytes());
        buf[4..8].copy_from_slice(&self.timestamp.seconds.to_le_bytes());
        buf[8..12].copy_from_slice(&self.timestamp.nanoseconds.to_le_bytes());
        buf[12..20].copy_from_slice(&self.additional_information.to_le_bytes());
        
        buf
    }
}