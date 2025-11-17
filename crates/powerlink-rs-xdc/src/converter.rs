// crates/powerlink-rs-xdc/src/converter.rs

//! Converts the public, schema-based `types` into the `powerlink-rs` core crate's
//! internal `od::ObjectDictionary` representation.

use crate::error::XdcError;
use crate::types;
use crate::types::XdcFile;
use alloc::boxed::Box;
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
    data_type_id: Option<&str>,
) -> Option<ValueRange> {
    match (low_limit_str, high_limit_str, data_type_id) {
        (Some(low_str), Some(high_str), Some(dt_id)) => {
            match (
                parse_string_to_value(low_str, dt_id),
                parse_string_to_value(high_str, dt_id),
            ) {
                (Some(min_val), Some(max_val)) => Some(ValueRange { min: min_val, max: max_val }),
                _ => {
                    warn!("Failed to parse low/high limit strings ('{}', '{}') for dataType {}", low_str, high_str, dt_id);
                    None
                }
            }
        }
        _ => None, // No range specified
    }
    // TODO: This only handles lowLimit/highLimit.
    // The next step is to also check obj.allowed_values and
    // merge the results if both are present.
}