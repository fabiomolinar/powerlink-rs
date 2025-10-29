// crates/powerlink-rs/src/od/pdo_validator.rs
use super::{Object, ObjectDictionary, ObjectValue};
use crate::{pdo::PdoMappingEntry, PowerlinkError};
use log::{error, trace};

/// Validates that a new PDO mapping configuration does not exceed payload size limits.
/// This should be called *before* writing to NumberOfEntries (sub-index 0) of a mapping object.
pub fn validate_pdo_mapping(
    od: &ObjectDictionary,
    index: u16,
    new_num_entries: u8,
) -> Result<(), PowerlinkError> {
    if new_num_entries == 0 {
        trace!(
            "PDO mapping {:#06X} deactivated (0 entries). Validation skipped.",
            index
        );
        return Ok(()); // Deactivating a mapping is always valid.
    }

    let is_tpdo = (0x1A00..=0x1AFF).contains(&index);
    let is_rpdo = (0x1600..=0x16FF).contains(&index);
    if !is_tpdo && !is_rpdo {
        error!(
            "validate_pdo_mapping called for non-PDO index {:#06X}",
            index
        );
        return Ok(());
    }

    // --- 1. Determine the HARD payload size limit for this PDO channel ---
    let payload_limit_bytes = if is_tpdo {
        // TPDOs (PRes on CN, PReq on MN) checked against IsochrTxMaxPayload_U16 (0x1F98/1).
        od.read_u16(0x1F98, 1).unwrap_or(1490) as usize
    } else {
        // RPDOs (PReq on CN, PRes on MN) checked against IsochrRxMaxPayload_U16 (0x1F98/2).
        od.read_u16(0x1F98, 2).unwrap_or(1490) as usize
    };

    // --- 2. Calculate the required size from the existing mapping entries ---
    let mut max_bits_required: u32 = 0;
    if let Some(Object::Array(entries)) = od.read_object(index) {
        for i in 0..(new_num_entries as usize) {
            if let Some(ObjectValue::Unsigned64(raw_mapping)) = entries.get(i) {
                let entry = PdoMappingEntry::from_u64(*raw_mapping);
                let end_pos_bits = entry.offset_bits as u32 + entry.length_bits as u32;
                max_bits_required = max_bits_required.max(end_pos_bits);
            } else {
                error!(
                    "PDO mapping validation error for {:#06X}: Trying to enable {} entries, but entry {} is missing.",
                    index,
                    new_num_entries,
                    i + 1
                );
                return Err(PowerlinkError::ValidationError(
                    "Incomplete PDO mapping configuration: Missing entries",
                ));
            }
        }
    } else {
        error!(
            "PDO mapping validation error: Object {:#06X} not found or not an Array.",
            index
        );
        return Err(PowerlinkError::ObjectNotFound);
    }

    let required_bytes = (max_bits_required + 7) / 8;

    // --- 3. Compare required size against the HARD limit and return result ---
    if required_bytes as usize > payload_limit_bytes {
        error!(
            "PDO mapping validation failed for index {:#06X}. Required size: {} bytes, Hard Limit: {} bytes. [E_PDO_MAP_OVERRUN]",
            index, required_bytes, payload_limit_bytes
        );
        Err(PowerlinkError::PdoMapOverrun)
    } else {
        trace!(
            "PDO mapping validation successful for index {:#06X}. Required: {} bytes, Hard Limit: {}",
            index, required_bytes, payload_limit_bytes
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::od::{AccessType, ObjectEntry};
    use alloc::vec;

    #[test]
    fn test_pdo_mapping_validation_success() {
        let mut od = ObjectDictionary::new(None);
        od.insert(
            0x1F98,
            ObjectEntry {
                object: Object::Record(vec![
                    ObjectValue::Unsigned16(100), // IsochrTxMaxPayload
                    ObjectValue::Unsigned16(0),
                ]),
                ..Default::default()
            },
        );
        let mapping1 = PdoMappingEntry {
            index: 0x6000,
            sub_index: 1,
            offset_bits: 0,
            length_bits: 8,
        };
        let mapping2 = PdoMappingEntry {
            index: 0x6001,
            sub_index: 0,
            offset_bits: 16,
            length_bits: 32,
        };
        od.insert(
            0x1A00,
            ObjectEntry {
                object: Object::Array(vec![
                    ObjectValue::Unsigned64(mapping1.to_u64()),
                    ObjectValue::Unsigned64(mapping2.to_u64()),
                ]),
                access: Some(AccessType::ReadWriteStore),
                ..Default::default()
            },
        );

        // Directly test the validator function
        let result = validate_pdo_mapping(&od, 0x1A00, 2);
        assert!(result.is_ok(), "Validation failed unexpectedly: {:?}", result);
    }

    #[test]
    fn test_pdo_mapping_validation_failure_hard_limit() {
        let mut od = ObjectDictionary::new(None);
        od.insert(
            0x1F98,
            ObjectEntry {
                object: Object::Record(vec![
                    ObjectValue::Unsigned16(10), // IsochrTxMaxPayload
                    ObjectValue::Unsigned16(0),
                ]),
                ..Default::default()
            },
        );
        let mapping1 = PdoMappingEntry {
            index: 0x6000,
            sub_index: 1,
            offset_bits: 0,
            length_bits: 64,
        };
        let mapping2 = PdoMappingEntry {
            index: 0x6001,
            sub_index: 0,
            offset_bits: 64,
            length_bits: 32,
        };
        od.insert(
            0x1A00,
            ObjectEntry {
                object: Object::Array(vec![
                    ObjectValue::Unsigned64(mapping1.to_u64()),
                    ObjectValue::Unsigned64(mapping2.to_u64()),
                ]),
                access: Some(AccessType::ReadWriteStore),
                ..Default::default()
            },
        );
        let result = validate_pdo_mapping(&od, 0x1A00, 2);
        assert!(matches!(result, Err(PowerlinkError::PdoMapOverrun)));
    }

    #[test]
    fn test_pdo_mapping_validation_failure_incomplete_map() {
        let mut od = ObjectDictionary::new(None);
        od.insert(
            0x1F98,
            ObjectEntry {
                object: Object::Record(vec![
                    ObjectValue::Unsigned16(100), // IsochrTxMaxPayload
                    ObjectValue::Unsigned16(0),
                ]),
                ..Default::default()
            },
        );
        let mapping1 = PdoMappingEntry {
            index: 0x6000,
            sub_index: 1,
            offset_bits: 0,
            length_bits: 8,
        };
        od.insert(
            0x1A00,
            ObjectEntry {
                object: Object::Array(vec![ObjectValue::Unsigned64(mapping1.to_u64())]),
                access: Some(AccessType::ReadWriteStore),
                ..Default::default()
            },
        );

        let result = validate_pdo_mapping(&od, 0x1A00, 2);
        assert!(matches!(
            result,
            Err(PowerlinkError::ValidationError(
                "Incomplete PDO mapping configuration: Missing entries"
            ))
        ));
    }

}