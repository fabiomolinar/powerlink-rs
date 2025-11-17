// crates/powerlink-rs-xdc/src/converter.rs

//! Converts the public, schema-based `types` into the `powerlink-rs` core crate's
//! internal `od::ObjectDictionary` representation.

use crate::error::XdcError;
use crate::types;
use crate::types::XdcFile;
use alloc::boxed::Box;
use alloc::collections::BTreeMap; // Added for storage map
use alloc::string::String; // Correctly import String
use alloc::vec::Vec;
use powerlink_rs::od::{
    AccessType, Category, Object, ObjectDictionary, ObjectEntry, ObjectValue, PdoMapping,
    ValueRange, // Import ValueRange
};
use log::warn; // Import warn

/// Converts a parsed `XdcFile` into the `ObjectDictionary` format required
/// by the `powerlink-rs` core crate.
///
/// This function is intended to be used with XDD files (loaded via
/// `load_xdd_defaults_from_str`) to create the *factory default* OD.
///
/// This is the primary integration point between the XDC parser and the
/// core POWERLINK stack.
pub fn to_core_od(xdc_file: &XdcFile) -> Result<ObjectDictionary<'static>, XdcError> {
    // Create a new, empty `ObjectDictionary` from the core crate.
    // We pass `None` for storage, as the XDC file *is* the storage.
    let mut core_od = ObjectDictionary::new(None);

    for obj in &xdc_file.object_dictionary.objects {
        // Convert the `types::Object` into a `powerlink_rs::od::Object`.
        let core_object = map_object(obj)?;

        // --- MODIFIED: Resolve ValueRange ---
        // This logic is now implemented.
        let value_range = resolve_value_range(
            obj.low_limit.as_deref(),
            obj.high_limit.as_deref(),
            obj.allowed_values.as_ref(), // Pass in the new field
            obj.data_type.as_deref(),
        );

        // Convert the metadata into a `powerlink_rs::od::ObjectEntry`.
        let core_entry = ObjectEntry {
            object: core_object,
            name: Box::leak(obj.name.clone().into_boxed_str()), // Leak string to get 'static str
            category: map_support_to_category(obj.support),
            access: obj.access_type.map(map_access_type),
            // The XDC file's data *is* the default value for the core.
            default_value: obj.data.as_ref().map(|d| map_data_to_value(d, obj.data_type.as_deref())).transpose()?.flatten(),
            value_range, // MODIFIED: Assign the resolved value range
            pdo_mapping: obj.pdo_mapping.map(map_pdo_mapping),
        };

        core_od.insert(obj.index, core_entry);
    }

    Ok(core_od)
}

/// Converts a parsed `XdcFile` into a `BTreeMap` suitable for use with
/// the `powerlink_rs::hal::ObjectDictionaryStorage` trait.
///
/// This function is intended to be used with XDC files (loaded via
/// `load_xdc_from_str`) to extract the `actualValue` data.
pub fn xdc_to_storage_map(
    xdc_file: &XdcFile,
) -> Result<BTreeMap<(u16, u8), ObjectValue>, XdcError> {
    let mut map = BTreeMap::new();

    for obj in &xdc_file.object_dictionary.objects {
        if obj.object_type == "7" {
            // This is a VAR. Data is on the object itself (sub-index 0).
            if let (Some(data), Some(data_type_id)) =
                (obj.data.as_ref(), obj.data_type.as_deref())
            {
                if let Some(value) = map_data_to_value(data, Some(data_type_id))? {
                    map.insert((obj.index, 0), value);
                }
            }
        } else {
            // This is a RECORD or ARRAY. Data is on the sub-objects.
            for sub_obj in &obj.sub_objects {
                if let (Some(data), Some(data_type_id)) =
                    (sub_obj.data.as_ref(), sub_obj.data_type.as_deref())
                {
                    if let Some(value) = map_data_to_value(data, Some(data_type_id))? {
                        map.insert((obj.index, sub_obj.sub_index), value);
                    }
                }
            }
        }
    }

    Ok(map)
}

/// Maps the public `types::Object` to the core `powerlink_rs::od::Object`.
fn map_object(obj: &types::Object) -> Result<Object, XdcError> {
    match obj.object_type.as_str() {
        "7" => {
            // VAR
            let value = obj
                .data
                .as_ref()
                .map(|d| map_data_to_value(d, obj.data_type.as_deref()))
                .transpose()?
                .flatten()
                .ok_or(XdcError::ValidationError(
                    "VAR object (7) missing data or dataType",
                ))?;
            Ok(Object::Variable(value))
        }
        "8" => {
            // ARRAY
            let mut sub_values = Vec::new();
            // Sub-index 0 is "NumberOfEntries"
            let num_entries = obj
                .sub_objects
                .iter()
                .find(|so| so.sub_index == 0)
                .and_then(|so| so.data.as_ref())
                .and_then(|d| d.first())
                .copied()
                .unwrap_or(0);
            
            // Pre-allocate based on sub-index 0
            sub_values.resize(num_entries as usize, ObjectValue::Unsigned8(0)); // Fill with dummy data

            for sub_obj in &obj.sub_objects {
                if sub_obj.sub_index == 0 {
                    continue;
                }
                let value = sub_obj.data.as_ref()
                    .map(|d| map_data_to_value(d, sub_obj.data_type.as_deref()))
                    .transpose()?
                    .flatten()
                    .ok_or(XdcError::ValidationError("Array sub-object missing data"))?;
                
                let idx = sub_obj.sub_index as usize - 1;
                if let Some(slot) = sub_values.get_mut(idx) {
                    *slot = value;
                }
            }
            Ok(Object::Array(sub_values))
        }
        "9" => {
            // RECORD
            let mut sub_values = Vec::new();
            // Sub-index 0 is "NumberOfEntries"
            let num_entries = obj
                .sub_objects
                .iter()
                .find(|so| so.sub_index == 0)
                .and_then(|so| so.data.as_ref())
                .and_then(|d| d.first())
                .copied()
                .unwrap_or(0);

            // Pre-allocate based on sub-index 0
            sub_values.resize(num_entries as usize, ObjectValue::Unsigned8(0)); // Fill with dummy data

            for sub_obj in &obj.sub_objects {
                if sub_obj.sub_index == 0 {
                    continue;
                }
                let value = sub_obj.data.as_ref()
                    .map(|d| map_data_to_value(d, sub_obj.data_type.as_deref()))
                    .transpose()?
                    .flatten()
                    .ok_or(XdcError::ValidationError("Record sub-object missing data"))?;
                
                let idx = sub_obj.sub_index as usize - 1;
                if let Some(slot) = sub_values.get_mut(idx) {
                    *slot = value;
                }
            }
            Ok(Object::Record(sub_values))
        }
        _ => Err(XdcError::ValidationError("Unknown objectType")),
    }
}

/// Maps a raw byte slice and a data type ID string to the core `ObjectValue` enum.
fn map_data_to_value(data: &[u8], data_type_id: Option<&str>) -> Result<Option<ObjectValue>, XdcError> {
    let id = match data_type_id {
        Some(id) => id,
        None => return Ok(None), // Cannot map without a type
    };

    // Helper macro to deserialize LE bytes
    macro_rules! from_le {
        ($typ:ty, $variant:path) => {
            data.try_into()
                .map(<$typ>::from_le_bytes)
                .map($variant)
                .map(Some)
                .map_err(|_| XdcError::ValidationError("Data length mismatch for type"))
        };
    }

    match id {
        "0001" => from_le!(u8, ObjectValue::Boolean),
        "0002" => from_le!(i8, ObjectValue::Integer8),
        "0003" => from_le!(i16, ObjectValue::Integer16),
        "0004" => from_le!(i32, ObjectValue::Integer32),
        "0005" => from_le!(u8, ObjectValue::Unsigned8),
        "0006" => from_le!(u16, ObjectValue::Unsigned16),
        "0007" => from_le!(u32, ObjectValue::Unsigned32),
        "0008" => from_le!(f32, ObjectValue::Real32),
        "0009" => Ok(Some(ObjectValue::VisibleString(
            String::from_utf8(data.to_vec()).map_err(|_| XdcError::ValidationError("Invalid UTF-8"))?,
        ))),
        "000A" => Ok(Some(ObjectValue::OctetString(data.to_vec()))),
        "000F" => Ok(Some(ObjectValue::Domain(data.to_vec()))),
        "0011" => from_le!(f64, ObjectValue::Real64),
        "0015" => from_le!(i64, ObjectValue::Integer64),
        "001B" => from_le!(u64, ObjectValue::Unsigned64),
        // Add more types as needed...
        _ => Ok(None), // Type not recognized or mapped yet
    }
}

/// Maps the XDC parser's `ParameterAccess` to the core crate's `AccessType`.
fn map_access_type(access: types::ParameterAccess) -> AccessType {
    match access {
        types::ParameterAccess::Constant => AccessType::Constant,
        types::ParameterAccess::ReadOnly => AccessType::ReadOnly,
        types::ParameterAccess::WriteOnly => AccessType::WriteOnly,
        types::ParameterAccess::ReadWrite => AccessType::ReadWrite,
        // Map ReadWriteStore and WriteOnlyStore to their non-storing counterparts
        // The `persistent` flag in `types::Object` will be used by the core
        // crate's CFM logic to know *what* to save, but the OD's access
        // type only cares about read/write permissions.
        types::ParameterAccess::ReadWriteInput => AccessType::ReadWrite,
        types::ParameterAccess::ReadWriteOutput => AccessType::ReadWrite,
        types::ParameterAccess::NoAccess => AccessType::ReadOnly, // NoAccess is effectively RO
    }
}

/// Maps the XDC parser's `ObjectPdoMapping` to the core crate's `PdoMapping`.
fn map_pdo_mapping(mapping: types::ObjectPdoMapping) -> PdoMapping {
    match mapping {
        types::ObjectPdoMapping::No => PdoMapping::No,
        types::ObjectPdoMapping::Default => PdoMapping::Default,
        types::ObjectPdoMapping::Optional => PdoMapping::Optional,
        // TPDO and RPDO are just special cases of "Optional" for the runtime
        types::ObjectPdoMapping::Tpdo => PdoMapping::Optional,
        types::ObjectPdoMapping::Rpdo => PdoMapping::Optional,
    }
}

/// Maps the XDC parser's `ParameterSupport` to the core crate's `Category`.
fn map_support_to_category(support: Option<types::ParameterSupport>) -> Category {
    match support {
        Some(types::ParameterSupport::Mandatory) => Category::Mandatory,
        Some(types::ParameterSupport::Optional) => Category::Optional,
        Some(types::ParameterSupport::Conditional) => Category::Conditional,
        None => Category::Optional, // Default to Optional if not specified
    }
}

/// --- NEW: Helper functions for ValueRange resolution ---

/// Parses a limit string (hex or decimal) into an `ObjectValue` based on type.
fn parse_string_to_value(s: &str, data_type_id: &str) -> Option<ObjectValue> {
    // Helper to parse, removing "0x" if present
    fn parse_hex<T: FromStrRadix>(s: &str) -> Option<T> {
        // MODIFIED: Use from_str_radix, not parse()
        T::from_str_radix(s.strip_prefix("0x").unwrap_or(s), 16).ok()
    }
    
    // Helper to parse, supporting "0x" hex or decimal
    fn parse_num<T: FromStrRadix + core::str::FromStr>(s: &str) -> Option<T> {
        if let Some(hex_str) = s.strip_prefix("0x") {
            T::from_str_radix(hex_str, 16).ok()
        } else {
            s.parse::<T>().ok()
        }
    }
    
    // Trait to unify from_str_radix and FromStr
    trait FromStrRadix: Sized {
        fn from_str_radix(src: &str, radix: u32) -> Result<Self, core::num::ParseIntError>;
    }
    impl FromStrRadix for i8 { fn from_str_radix(s: &str, r: u32) -> Result<Self, core::num::ParseIntError> { i8::from_str_radix(s, r) } }
    impl FromStrRadix for i16 { fn from_str_radix(s: &str, r: u32) -> Result<Self, core::num::ParseIntError> { i16::from_str_radix(s, r) } }
    impl FromStrRadix for i32 { fn from_str_radix(s: &str, r: u32) -> Result<Self, core::num::ParseIntError> { i32::from_str_radix(s, r) } }
    impl FromStrRadix for i64 { fn from_str_radix(s: &str, r: u32) -> Result<Self, core::num::ParseIntError> { i64::from_str_radix(s, r) } }
    impl FromStrRadix for u8 { fn from_str_radix(s: &str, r: u32) -> Result<Self, core::num::ParseIntError> { u8::from_str_radix(s, r) } }
    impl FromStrRadix for u16 { fn from_str_radix(s: &str, r: u32) -> Result<Self, core::num::ParseIntError> { u16::from_str_radix(s, r) } }
    impl FromStrRadix for u32 { fn from_str_radix(s: &str, r: u32) -> Result<Self, core::num::ParseIntError> { u32::from_str_radix(s, r) } }
    impl FromStrRadix for u64 { fn from_str_radix(s: &str, r: u32) -> Result<Self, core::num::ParseIntError> { u64::from_str_radix(s, r) } }

    match data_type_id {
        "0001" => parse_num::<u8>(s).map(ObjectValue::Boolean),
        "0002" => parse_num::<i8>(s).map(ObjectValue::Integer8),
        "0003" => parse_num::<i16>(s).map(ObjectValue::Integer16),
        "0004" => parse_num::<i32>(s).map(ObjectValue::Integer32),
        "0005" => parse_num::<u8>(s).map(ObjectValue::Unsigned8),
        "0006" => parse_num::<u16>(s).map(ObjectValue::Unsigned16),
        "0007" => parse_num::<u32>(s).map(ObjectValue::Unsigned32),
        "0008" => s.parse::<f32>().ok().map(ObjectValue::Real32),
        // 0009 (VisibleString) and 000A (OctetString) don't typically have numeric ranges
        "0011" => s.parse::<f64>().ok().map(ObjectValue::Real64),
        "0015" => parse_num::<i64>(s).map(ObjectValue::Integer64),
        "001B" => parse_num::<u64>(s).map(ObjectValue::Unsigned64),
        // Other types (Integer24, Domain, etc.) are not handled for ranges yet
        _ => {
            warn!("ValueRange parsing not implemented for dataType {}", data_type_id);
            None
        }
    }
}

/// Resolves the `ValueRange` for an object.
/// This is a new helper function.
fn resolve_value_range(
    low_limit_str: Option<&str>,
    high_limit_str: Option<&str>,
    allowed_values: Option<&types::AllowedValues>, // MODIFIED: Added parameter
    data_type_id: Option<&str>,
) -> Option<ValueRange> {
    let dt_id = match data_type_id {
        Some(id) => id,
        None => return None, // Cannot parse range without a type
    };

    // Priority 1: Direct lowLimit/highLimit attributes on the <Object> or <SubObject>.
    if let (Some(low_str), Some(high_str)) = (low_limit_str, high_limit_str) {
        match (
            parse_string_to_value(low_str, dt_id),
            parse_string_to_value(high_str, dt_id),
        ) {
            (Some(min_val), Some(max_val)) => {
                return Some(ValueRange { min: min_val, max: max_val });
            }
            _ => {
                warn!("Failed to parse low/high limit strings ('{}', '{}') for dataType {}", low_str, high_str, dt_id);
                // Fall through to check allowedValues
            }
        }
    }

    // Priority 2: <allowedValues> from the resolved <parameter>.
    if let Some(av) = allowed_values {
        // The core `ValueRange` struct only supports min/max, not enumerated values.
        // We will use the *first* <range> element we find.
        if let Some(range) = av.ranges.first() {
            match (
                parse_string_to_value(&range.min_value, dt_id),
                parse_string_to_value(&range.max_value, dt_id),
            ) {
                (Some(min_val), Some(max_val)) => {
                    return Some(ValueRange { min: min_val, max: max_val });
                }
                _ => {
                    warn!("Failed to parse <range> min/max strings ('{}', '{}') for dataType {}", range.min_value, range.max_value, dt_id);
                }
            }
        }
        // TODO: The core `ValueRange` could be extended to support enumerated `allowedValues.values`.
        // For now, we only support <range>.
    }

    None // No range specified or found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SubObject; // Use the public types
    use alloc::string::ToString;
    use alloc::vec;
    use powerlink_rs::od::{AccessType, Category, Object, ObjectValue, PdoMapping}; // Import the core OD Object enum

    #[test]
    fn test_to_core_od_var_conversion() {
        let xdc_file = types::XdcFile {
            object_dictionary: types::ObjectDictionary {
                objects: vec![crate::types::Object {
                    index: 0x1000,
                    name: "Device Type".to_string(),
                    object_type: "7".to_string(),
                    data_type: Some("0007".to_string()), // Unsigned32
                    access_type: Some(types::ParameterAccess::Constant),
                    pdo_mapping: Some(types::ObjectPdoMapping::No),
                    support: Some(types::ParameterSupport::Mandatory),
                    persistent: false,
                    data: Some(vec![0x91, 0x01, 0x0F, 0x00]), // 0x000F0191_u32.to_le_bytes()
                    ..Default::default()
                }],
            },
            ..Default::default()
        };

        let core_od = to_core_od(&xdc_file).unwrap();

        // Use the public API of the core OD to check the value
        let entry = core_od.read_object(0x1000).unwrap();
        if let Object::Variable(val) = entry {
            assert_eq!(*val, ObjectValue::Unsigned32(0x000F0191));
        } else {
            panic!("Expected Object::Variable");
        }
        
        // We can't check metadata easily as `entries` is private.
        // This test now correctly verifies the data conversion.
    }

    #[test]
    fn test_to_core_od_record_conversion() {
        let xdc_file = types::XdcFile {
            object_dictionary: types::ObjectDictionary {
                objects: vec![crate::types::Object {
                    index: 0x1018,
                    name: "Identity".to_string(),
                    object_type: "9".to_string(), // RECORD
                    sub_objects: vec![
                        SubObject {
                            sub_index: 0,
                            name: "Count".to_string(),
                            object_type: "7".to_string(),
                            data_type: Some("0005".to_string()), // Unsigned8
                            data: Some(vec![2]), // Number of entries
                            ..Default::default()
                        },
                        SubObject {
                            sub_index: 1,
                            name: "VendorID".to_string(),
                            object_type: "7".to_string(),
                            data_type: Some("0007".to_string()), // Unsigned32
                            data: Some(vec![0x78, 0x56, 0x34, 0x12]),
                            ..Default::default()
                        },
                        SubObject {
                            sub_index: 2,
                            name: "ProductCode".to_string(),
                            object_type: "7".to_string(),
                            data_type: Some("0007".to_string()), // Unsigned32
                            data: Some(vec![0x34, 0x12, 0x00, 0x00]),
                            ..Default::default()
                        },
                    ],
                    ..Default::default()
                }],
            },
            ..Default::default()
        };

        let core_od = to_core_od(&xdc_file).unwrap();
        
        // Use the public API of the core OD to check the values
        let entry = core_od.read_object(0x1018).unwrap();
        if let Object::Record(vals) = entry {
            assert_eq!(vals.len(), 2);
            assert_eq!(vals[0], ObjectValue::Unsigned32(0x12345678));
            assert_eq!(vals[1], ObjectValue::Unsigned32(0x00001234));
        } else {
            panic!("Expected Object::Record");
        }
    }

    #[test]
    fn test_to_core_od_array_conversion() {
        let xdc_file = types::XdcFile {
            object_dictionary: types::ObjectDictionary {
                objects: vec![crate::types::Object {
                    index: 0x2000,
                    name: "MyArray".to_string(),
                    object_type: "8".to_string(), // ARRAY
                    sub_objects: vec![
                        SubObject {
                            sub_index: 0,
                            name: "Count".to_string(),
                            data: Some(vec![3]), // 3 entries
                            ..Default::default()
                        },
                        SubObject {
                            sub_index: 1,
                            name: "Val1".to_string(),
                            data_type: Some("0006".to_string()), // U16
                            data: Some(vec![0x11, 0x22]),
                            ..Default::default()
                        },
                        SubObject {
                            sub_index: 2,
                            name: "Val2".to_string(),
                            data_type: Some("0006".to_string()),
                            data: Some(vec![0xAA, 0xBB]),
                            ..Default::default()
                        },
                        // Entry 3 is missing, should be dummy
                    ],
                    ..Default::default()
                }],
            },
            ..Default::default()
        };

        let core_od = to_core_od(&xdc_file).unwrap();
        let entry = core_od.read_object(0x2000).unwrap();

        // Check that the array is created with the correct length (from sub-index 0)
        // and that missing entries are filled with dummy data.
        if let Object::Array(vals) = entry {
            assert_eq!(vals.len(), 3);
            assert_eq!(vals[0], ObjectValue::Unsigned16(0x2211));
            assert_eq!(vals[1], ObjectValue::Unsigned16(0xBBAA));
            assert_eq!(vals[2], ObjectValue::Unsigned8(0)); // Dummy data
        } else {
            panic!("Expected Object::Array");
        }
    }

    #[test]
    fn test_map_data_to_value() {
        // Test all fixed-size types
        assert_eq!(
            map_data_to_value(&[0x01], Some("0001")).unwrap().unwrap(),
            ObjectValue::Boolean(1)
        );
        assert_eq!(
            map_data_to_value(&[0x80], Some("0002")).unwrap().unwrap(),
            ObjectValue::Integer8(-128)
        );
        assert_eq!(
            map_data_to_value(&[0xFE, 0xFF], Some("0003")).unwrap().unwrap(),
            ObjectValue::Integer16(-2)
        );
        assert_eq!(
            map_data_to_value(&[0x01, 0x00, 0x00, 0x80], Some("0004")).unwrap().unwrap(),
            ObjectValue::Integer32(-2147483647)
        );
        assert_eq!(
            map_data_to_value(&[0xFE, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x7F], Some("0015")).unwrap().unwrap(),
            ObjectValue::Integer64(i64::MAX - 1)
        );
        assert_eq!(
            map_data_to_value(&[0x42], Some("0005")).unwrap().unwrap(),
            ObjectValue::Unsigned8(0x42)
        );
        assert_eq!(
            map_data_to_value(&[0x34, 0x12], Some("0006")).unwrap().unwrap(),
            ObjectValue::Unsigned16(0x1234)
        );
        assert_eq!(
            map_data_to_value(&[0x78, 0x56, 0x34, 0x12], Some("0007")).unwrap().unwrap(),
            ObjectValue::Unsigned32(0x12345678)
        );
        assert_eq!(
            map_data_to_value(&[0x44, 0x33, 0x22, 0x11, 0xEF, 0xCD, 0xAB, 0x89], Some("001B")).unwrap().unwrap(),
            ObjectValue::Unsigned64(0x89ABCDEF11223344)
        );
        assert_eq!(
            map_data_to_value(&[0x00, 0x00, 0xC0, 0x3F], Some("0008")).unwrap().unwrap(),
            ObjectValue::Real32(1.5)
        );
        assert_eq!(
            map_data_to_value(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xF8, 0x3F], Some("0011")).unwrap().unwrap(),
            ObjectValue::Real64(1.5)
        );

        // Test variable-size types
        let vs_data = vec![0x48, 0x65, 0x6C, 0x6C, 0x6F]; // "Hello"
        assert_eq!(
            map_data_to_value(&vs_data, Some("0009")).unwrap().unwrap(),
            ObjectValue::VisibleString("Hello".to_string())
        );
        let os_data = vec![0x00, 0xDE, 0xAD, 0x00, 0xBE, 0xEF];
        assert_eq!(
            map_data_to_value(&os_data, Some("000A")).unwrap().unwrap(),
            ObjectValue::OctetString(os_data.clone())
        );
        assert_eq!(
            map_data_to_value(&os_data, Some("000F")).unwrap().unwrap(),
            ObjectValue::Domain(os_data.clone())
        );

        // Test error cases
        assert!(matches!(
            map_data_to_value(&[0x01, 0x02], Some("0005")), // Expected 1 byte, got 2
            Err(XdcError::ValidationError("Data length mismatch for type"))
        ));
        assert!(matches!(
            map_data_to_value(&[0xFF], Some("0009")), // Invalid UTF-8
            Err(XdcError::ValidationError("Invalid UTF-8"))
        ));
        
        // Test unknown type
        assert_eq!(
            map_data_to_value(&[0x01, 0x02], Some("9999")).unwrap(),
            None
        );
    }

    #[test]
    fn test_map_access_type() {
        use types::ParameterAccess as Public;
        assert_eq!(map_access_type(Public::Constant), AccessType::Constant);
        assert_eq!(map_access_type(Public::ReadOnly), AccessType::ReadOnly);
        assert_eq!(map_access_type(Public::WriteOnly), AccessType::WriteOnly);
        assert_eq!(map_access_type(Public::ReadWrite), AccessType::ReadWrite);
        assert_eq!(map_access_type(Public::ReadWriteInput), AccessType::ReadWrite);
        assert_eq!(map_access_type(Public::ReadWriteOutput), AccessType::ReadWrite);
        assert_eq!(map_access_type(Public::NoAccess), AccessType::ReadOnly);
    }

    #[test]
    fn test_map_pdo_mapping() {
        use types::ObjectPdoMapping as Public;
        assert_eq!(map_pdo_mapping(Public::No), PdoMapping::No);
        assert_eq!(map_pdo_mapping(Public::Default), PdoMapping::Default);
        assert_eq!(map_pdo_mapping(Public::Optional), PdoMapping::Optional);
        assert_eq!(map_pdo_mapping(Public::Tpdo), PdoMapping::Optional);
        assert_eq!(map_pdo_mapping(Public::Rpdo), PdoMapping::Optional);
    }

    #[test]
    fn test_map_support_to_category() {
        use types::ParameterSupport as Public;
        assert_eq!(map_support_to_category(Some(Public::Mandatory)), Category::Mandatory);
        assert_eq!(map_support_to_category(Some(Public::Optional)), Category::Optional);
        assert_eq!(map_support_to_category(Some(Public::Conditional)), Category::Conditional);
        assert_eq!(map_support_to_category(None), Category::Optional);
    }

    #[test]
    fn test_xdc_to_storage_map() {
        let xdc_file = types::XdcFile {
            object_dictionary: types::ObjectDictionary {
                objects: vec![
                    // VAR (Sub-index 0)
                    crate::types::Object {
                        index: 0x1006,
                        name: "NMT_CycleLen_U32".to_string(),
                        object_type: "7".to_string(),
                        data_type: Some("0007".to_string()), // U32
                        data: Some(vec![0x10, 0x27, 0x00, 0x00]), // 10000_u32.to_le_bytes()
                        ..Default::default()
                    },
                    // RECORD (Sub-indices 0, 1, 2)
                    crate::types::Object {
                        index: 0x1018,
                        name: "Identity".to_string(),
                        object_type: "9".to_string(),
                        sub_objects: vec![
                            SubObject {
                                sub_index: 0,
                                name: "Count".to_string(),
                                object_type: "7".to_string(),
                                data_type: Some("0005".to_string()), // U8
                                data: Some(vec![2]), // Number of entries
                                ..Default::default()
                            },
                            SubObject {
                                sub_index: 1,
                                name: "VendorID".to_string(),
                                object_type: "7".to_string(),
                                data_type: Some("0007".to_string()), // U32
                                data: Some(vec![0x78, 0x56, 0x34, 0x12]),
                                ..Default::default()
                            },
                            SubObject {
                                sub_index: 2,
                                name: "ProductCode".to_string(),
                                object_type: "7".to_string(),
                                data_type: Some("0007".to_string()), // U32
                                data: Some(vec![0x34, 0x12, 0x00, 0x00]),
                                ..Default::default()
                            },
                        ],
                        ..Default::default()
                    },
                    // ARRAY (Sub-indices 0, 1)
                    crate::types::Object {
                        index: 0x2000,
                        name: "MyArray".to_string(),
                        object_type: "8".to_string(),
                        sub_objects: vec![
                            SubObject {
                                sub_index: 0,
                                name: "Count".to_string(),
                                data_type: Some("0005".to_string()),
                                data: Some(vec![1]),
                                ..Default::default()
                            },
                            SubObject {
                                sub_index: 1,
                                name: "Val1".to_string(),
                                data_type: Some("0006".to_string()), // U16
                                data: Some(vec![0x11, 0x22]),
                                ..Default::default()
                            },
                        ],
                        ..Default::default()
                    },
                ],
            },
            ..Default::default()
        };

        let map = xdc_to_storage_map(&xdc_file).unwrap();

        // Should contain 5 entries:
        // 0x1006/0 (VAR)
        // 0x1018/0 (RECORD Sub-object)
        // 0x1018/1 (RECORD Sub-object)
        // 0x1018/2 (RECORD Sub-object)
        // 0x2000/0 (ARRAY Sub-object)
        // 0x2000/1 (ARRAY Sub-object)
        assert_eq!(map.len(), 6);

        // Check VAR
        assert_eq!(
            map.get(&(0x1006, 0)),
            Some(&ObjectValue::Unsigned32(10000))
        );

        // Check RECORD
        assert_eq!(
            map.get(&(0x1018, 0)),
            Some(&ObjectValue::Unsigned8(2))
        );
        assert_eq!(
            map.get(&(0x1018, 1)),
            Some(&ObjectValue::Unsigned32(0x12345678))
        );
        assert_eq!(
            map.get(&(0x1018, 2)),
            Some(&ObjectValue::Unsigned32(0x00001234))
        );

        // Check ARRAY
         assert_eq!(
            map.get(&(0x2000, 0)),
            Some(&ObjectValue::Unsigned8(1))
        );
        assert_eq!(
            map.get(&(0x2000, 1)),
            Some(&ObjectValue::Unsigned16(0x2211))
        );
    }
}