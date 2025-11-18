// crates/powerlink-rs-xdc/src/converter.rs

//! Converts the public, schema-based `types` into the `powerlink-rs` core crate's
//! internal `od::ObjectDictionary` representation.

use crate::error::XdcError;
use crate::types;
use crate::types::XdcFile;
use crate::resolver::utils; // Import utils for type checking
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use log::warn;
use powerlink_rs::od::{
    AccessType,
    Category,
    Object,
    ObjectDictionary,
    ObjectEntry,
    ObjectValue,
    PdoMapping,
    ValueRange,
};
use powerlink_rs::nmt::flags::FeatureFlags; // Import core FeatureFlags

// Import DataTypeName for strong typing in map_data_to_value
use crate::model::app_layers::DataTypeName;

/// Configuration settings for the NMT state machine, extracted from the XDC profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NmtSettings {
    /// The calculated FeatureFlags (mapping to OD 0x1F82).
    pub feature_flags: FeatureFlags,
    /// The boot time not active duration in microseconds (mapping to NMTBootTimeNotActive).
    pub boot_time_not_active: u32,
    /// The minimum cycle time in microseconds.
    pub cycle_time_min: u32,
    /// The maximum cycle time in microseconds.
    pub cycle_time_max: u32,
}

/// Extracts NMT configuration settings from the `NetworkManagement` block of an XDC file.
///
/// This function performs the logic usually done by configuration tools: it aggregates
/// boolean attributes from `GeneralFeatures`, `CNFeatures`, and `MNFeatures` into
/// the single `FeatureFlags` bitmask used by the POWERLINK stack.
pub fn extract_nmt_settings(xdc_file: &XdcFile) -> Result<NmtSettings, XdcError> {
    let nm = xdc_file.network_management.as_ref().ok_or(XdcError::ValidationError(
        "XDC file missing required <NetworkManagement> block",
    ))?;

    let gf = &nm.general_features;

    let mut flags = FeatureFlags::empty();

    // --- Map GeneralFeatures ---
    if gf.nmt_isochronous.unwrap_or(true) { // Default true per spec/schema
        flags.insert(FeatureFlags::ISOCHRONOUS);
    }
    if gf.sdo_support_udp_ip.unwrap_or(false) {
        flags.insert(FeatureFlags::SDO_UDP);
    }
    if gf.sdo_support_asnd.unwrap_or(false) {
        flags.insert(FeatureFlags::SDO_ASND);
    }
    if gf.sdo_support_pdo.unwrap_or(false) {
        flags.insert(FeatureFlags::SDO_PDO);
    }
    if gf.nmt_ext_nmt_cmds.unwrap_or(false) {
        flags.insert(FeatureFlags::EXTENDED_NMT_CMDS);
    }
    if gf.pdo_dynamic_mapping.unwrap_or(true) {
         flags.insert(FeatureFlags::DYNAMIC_PDO_MAPPING);
    }
    if gf.cfm_config_manager.unwrap_or(false) {
        flags.insert(FeatureFlags::CONFIG_MANAGER);
    }
    if gf.nmt_node_id_by_sw.unwrap_or(false) {
        flags.insert(FeatureFlags::NODE_ID_BY_SW);
    }
    if gf.sdo_cmd_read_all_by_index.unwrap_or(false) || gf.sdo_cmd_write_all_by_index.unwrap_or(false) {
        flags.insert(FeatureFlags::SDO_RW_ALL_BY_INDEX);
    }
    if gf.sdo_cmd_read_mult_param.unwrap_or(false) || gf.sdo_cmd_write_mult_param.unwrap_or(false) {
        flags.insert(FeatureFlags::SDO_RW_MULTIPLE_BY_INDEX);
    }

    // --- Map MNFeatures ---
    if let Some(mnf) = &nm.mn_features {
        if mnf.nmt_service_udp_ip.unwrap_or(false) {
            flags.insert(FeatureFlags::NMT_SERVICE_UDP);
        }
        if mnf.dll_mn_feature_multiplex.unwrap_or(true) {
             flags.insert(FeatureFlags::MULTIPLEXED_ACCESS);
        }
        if mnf.nmt_mn_basic_ethernet.unwrap_or(false) {
            flags.insert(FeatureFlags::MN_BASIC_ETHERNET);
        }
        // NMT Info Services bit is often implied by the device class or specific attributes,
        // but for now we can map it if any NMT Publish features are active in GeneralFeatures.
        if gf.nmt_publish_active_nodes.unwrap_or(false) || gf.nmt_publish_config_nodes.unwrap_or(false) {
            flags.insert(FeatureFlags::NMT_INFO_SERVICES);
        }
    }

    // --- Map CNFeatures ---
    if let Some(cnf) = &nm.cn_features {
         if cnf.dll_cn_feature_multiplex.unwrap_or(false) {
             flags.insert(FeatureFlags::MULTIPLEXED_ACCESS);
         }
    }

    Ok(NmtSettings {
        feature_flags: flags,
        boot_time_not_active: gf.nmt_boot_time_not_active,
        cycle_time_min: gf.nmt_cycle_time_min,
        cycle_time_max: gf.nmt_cycle_time_max,
    })
}

/// Converts a parsed `XdcFile` into the `ObjectDictionary` format required
/// by the `powerlink-rs` core crate.
pub fn to_core_od(xdc_file: &XdcFile) -> Result<ObjectDictionary<'static>, XdcError> {
    let mut core_od = ObjectDictionary::new(None);

    for obj in &xdc_file.object_dictionary.objects {
        let core_object = map_object(obj)?;

        let value_range = resolve_value_range(
            obj.low_limit.as_deref(),
            obj.high_limit.as_deref(),
            obj.allowed_values.as_ref(),
            obj.data_type.as_deref(),
        );

        let core_entry = ObjectEntry {
            object: core_object,
            name: Box::leak(obj.name.clone().into_boxed_str()),
            category: map_support_to_category(obj.support),
            access: obj.access_type.map(map_access_type),
            default_value: obj
                .data
                .as_ref()
                .map(|d| map_data_to_value(d, obj.data_type.as_deref()))
                .transpose()?
                .flatten(),
            value_range,
            pdo_mapping: obj.pdo_mapping.map(map_pdo_mapping),
        };

        core_od.insert(obj.index, core_entry);
    }

    Ok(core_od)
}

/// Converts a parsed `XdcFile` into a `BTreeMap` suitable for use with
/// the `powerlink_rs::hal::ObjectDictionaryStorage` trait.
pub fn xdc_to_storage_map(
    xdc_file: &XdcFile,
) -> Result<BTreeMap<(u16, u8), ObjectValue>, XdcError> {
    let mut map = BTreeMap::new();

    for obj in &xdc_file.object_dictionary.objects {
        if obj.object_type == "7" {
            if let (Some(data), Some(data_type_id)) = (obj.data.as_ref(), obj.data_type.as_deref())
            {
                if let Some(value) = map_data_to_value(data, Some(data_type_id))? {
                    map.insert((obj.index, 0), value);
                }
            }
        } else {
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
            let num_entries = obj
                .sub_objects
                .iter()
                .find(|so| so.sub_index == 0)
                .and_then(|so| so.data.as_ref())
                .and_then(|d| d.first())
                .copied()
                .unwrap_or(0);

            sub_values.resize(num_entries as usize, ObjectValue::Unsigned8(0));

            for sub_obj in &obj.sub_objects {
                if sub_obj.sub_index == 0 {
                    continue;
                }
                let value = sub_obj
                    .data
                    .as_ref()
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
            let num_entries = obj
                .sub_objects
                .iter()
                .find(|so| so.sub_index == 0)
                .and_then(|so| so.data.as_ref())
                .and_then(|d| d.first())
                .copied()
                .unwrap_or(0);

            sub_values.resize(num_entries as usize, ObjectValue::Unsigned8(0));

            for sub_obj in &obj.sub_objects {
                if sub_obj.sub_index == 0 {
                    continue;
                }
                let value = sub_obj
                    .data
                    .as_ref()
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
///
/// Refactored to use `DataTypeName` enum matching for robustness.
fn map_data_to_value(
    data: &[u8],
    data_type_id: Option<&str>,
) -> Result<Option<ObjectValue>, XdcError> {
    let id_str = match data_type_id {
        Some(id) => id,
        None => return Ok(None),
    };

    // Resolve the hex ID string to a strong Enum
    let type_name = match utils::get_standard_type_from_hex(id_str) {
        Some(t) => t,
        None => return Ok(None), // Unknown or custom type, skip mapping
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

    // Match on the Enum instead of string literals
    match type_name {
        DataTypeName::Boolean => from_le!(u8, ObjectValue::Boolean),
        DataTypeName::Integer8 => from_le!(i8, ObjectValue::Integer8),
        DataTypeName::Integer16 => from_le!(i16, ObjectValue::Integer16),
        DataTypeName::Integer32 => from_le!(i32, ObjectValue::Integer32),
        DataTypeName::Unsigned8 => from_le!(u8, ObjectValue::Unsigned8),
        DataTypeName::Unsigned16 => from_le!(u16, ObjectValue::Unsigned16),
        DataTypeName::Unsigned32 => from_le!(u32, ObjectValue::Unsigned32),
        DataTypeName::Real32 => from_le!(f32, ObjectValue::Real32),
        DataTypeName::VisibleString => Ok(Some(ObjectValue::VisibleString(
            String::from_utf8(data.to_vec())
                .map_err(|_| XdcError::ValidationError("Invalid UTF-8"))?,
        ))),
        DataTypeName::OctetString => Ok(Some(ObjectValue::OctetString(data.to_vec()))),
        DataTypeName::Domain => Ok(Some(ObjectValue::Domain(data.to_vec()))),
        DataTypeName::Real64 => from_le!(f64, ObjectValue::Real64),
        DataTypeName::Integer64 => from_le!(i64, ObjectValue::Integer64),
        DataTypeName::Unsigned64 => from_le!(u64, ObjectValue::Unsigned64),
        // Add other mappings as needed...
        _ => Ok(None),
    }
}

fn map_access_type(access: types::ParameterAccess) -> AccessType {
    match access {
        types::ParameterAccess::Constant => AccessType::Constant,
        types::ParameterAccess::ReadOnly => AccessType::ReadOnly,
        types::ParameterAccess::WriteOnly => AccessType::WriteOnly,
        types::ParameterAccess::ReadWrite => AccessType::ReadWrite,
        types::ParameterAccess::ReadWriteInput => AccessType::ReadWrite,
        types::ParameterAccess::ReadWriteOutput => AccessType::ReadWrite,
        types::ParameterAccess::NoAccess => AccessType::ReadOnly,
    }
}

fn map_pdo_mapping(mapping: types::ObjectPdoMapping) -> PdoMapping {
    match mapping {
        types::ObjectPdoMapping::No => PdoMapping::No,
        types::ObjectPdoMapping::Default => PdoMapping::Default,
        types::ObjectPdoMapping::Optional => PdoMapping::Optional,
        types::ObjectPdoMapping::Tpdo => PdoMapping::Optional,
        types::ObjectPdoMapping::Rpdo => PdoMapping::Optional,
    }
}

fn map_support_to_category(support: Option<types::ParameterSupport>) -> Category {
    match support {
        Some(types::ParameterSupport::Mandatory) => Category::Mandatory,
        Some(types::ParameterSupport::Optional) => Category::Optional,
        Some(types::ParameterSupport::Conditional) => Category::Conditional,
        None => Category::Optional,
    }
}

// --- Helper functions for ValueRange resolution ---

fn parse_string_to_value(s: &str, data_type_id: &str) -> Option<ObjectValue> {
    // Helper to parse, supporting "0x" hex or decimal
    fn parse_num<T: FromStrRadix + core::str::FromStr>(s: &str) -> Option<T> {
        if let Some(hex_str) = s.strip_prefix("0x") {
            T::from_str_radix(hex_str, 16).ok()
        } else {
            s.parse::<T>().ok()
        }
    }

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

    let type_name = utils::get_standard_type_from_hex(data_type_id)?;

    match type_name {
        DataTypeName::Boolean => parse_num::<u8>(s).map(ObjectValue::Boolean),
        DataTypeName::Integer8 => parse_num::<i8>(s).map(ObjectValue::Integer8),
        DataTypeName::Integer16 => parse_num::<i16>(s).map(ObjectValue::Integer16),
        DataTypeName::Integer32 => parse_num::<i32>(s).map(ObjectValue::Integer32),
        DataTypeName::Unsigned8 => parse_num::<u8>(s).map(ObjectValue::Unsigned8),
        DataTypeName::Unsigned16 => parse_num::<u16>(s).map(ObjectValue::Unsigned16),
        DataTypeName::Unsigned32 => parse_num::<u32>(s).map(ObjectValue::Unsigned32),
        DataTypeName::Real32 => s.parse::<f32>().ok().map(ObjectValue::Real32),
        DataTypeName::Real64 => s.parse::<f64>().ok().map(ObjectValue::Real64),
        DataTypeName::Integer64 => parse_num::<i64>(s).map(ObjectValue::Integer64),
        DataTypeName::Unsigned64 => parse_num::<u64>(s).map(ObjectValue::Unsigned64),
        _ => {
            warn!("ValueRange parsing not implemented for dataType {}", data_type_id);
            None
        }
    }
}

fn resolve_value_range(
    low_limit_str: Option<&str>,
    high_limit_str: Option<&str>,
    allowed_values: Option<&types::AllowedValues>,
    data_type_id: Option<&str>,
) -> Option<ValueRange> {
    let dt_id = match data_type_id {
        Some(id) => id,
        None => return None,
    };

    if let (Some(low_str), Some(high_str)) = (low_limit_str, high_limit_str) {
        match (
            parse_string_to_value(low_str, dt_id),
            parse_string_to_value(high_str, dt_id),
        ) {
            (Some(min_val), Some(max_val)) => {
                return Some(ValueRange {
                    min: min_val,
                    max: max_val,
                });
            }
            _ => {
                warn!(
                    "Failed to parse low/high limit strings ('{}', '{}') for dataType {}",
                    low_str, high_str, dt_id
                );
            }
        }
    }

    if let Some(av) = allowed_values {
        if let Some(range) = av.ranges.first() {
            match (
                parse_string_to_value(&range.min_value, dt_id),
                parse_string_to_value(&range.max_value, dt_id),
            ) {
                (Some(min_val), Some(max_val)) => {
                    return Some(ValueRange {
                        min: min_val,
                        max: max_val,
                    });
                }
                _ => {
                    warn!(
                        "Failed to parse <range> min/max strings ('{}', '{}') for dataType {}",
                        range.min_value, range.max_value, dt_id
                    );
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        GeneralFeatures, MnFeatures, NetworkManagement, XdcFile, SubObject
    };
    use powerlink_rs::nmt::flags::FeatureFlags;
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
                            data: Some(vec![2]),                 // Number of entries
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
            map_data_to_value(&[0xFE, 0xFF], Some("0003"))
                .unwrap()
                .unwrap(),
            ObjectValue::Integer16(-2)
        );
        assert_eq!(
            map_data_to_value(&[0x01, 0x00, 0x00, 0x80], Some("0004"))
                .unwrap()
                .unwrap(),
            ObjectValue::Integer32(-2147483647)
        );
        assert_eq!(
            map_data_to_value(
                &[0xFE, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x7F],
                Some("0015")
            )
            .unwrap()
            .unwrap(),
            ObjectValue::Integer64(i64::MAX - 1)
        );
        assert_eq!(
            map_data_to_value(&[0x42], Some("0005")).unwrap().unwrap(),
            ObjectValue::Unsigned8(0x42)
        );
        assert_eq!(
            map_data_to_value(&[0x34, 0x12], Some("0006"))
                .unwrap()
                .unwrap(),
            ObjectValue::Unsigned16(0x1234)
        );
        assert_eq!(
            map_data_to_value(&[0x78, 0x56, 0x34, 0x12], Some("0007"))
                .unwrap()
                .unwrap(),
            ObjectValue::Unsigned32(0x12345678)
        );
        assert_eq!(
            map_data_to_value(
                &[0x44, 0x33, 0x22, 0x11, 0xEF, 0xCD, 0xAB, 0x89],
                Some("001B")
            )
            .unwrap()
            .unwrap(),
            ObjectValue::Unsigned64(0x89ABCDEF11223344)
        );
        assert_eq!(
            map_data_to_value(&[0x00, 0x00, 0xC0, 0x3F], Some("0008"))
                .unwrap()
                .unwrap(),
            ObjectValue::Real32(1.5)
        );
        assert_eq!(
            map_data_to_value(
                &[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xF8, 0x3F],
                Some("0011")
            )
            .unwrap()
            .unwrap(),
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
        assert_eq!(
            map_access_type(Public::ReadWriteInput),
            AccessType::ReadWrite
        );
        assert_eq!(
            map_access_type(Public::ReadWriteOutput),
            AccessType::ReadWrite
        );
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
        assert_eq!(
            map_support_to_category(Some(Public::Mandatory)),
            Category::Mandatory
        );
        assert_eq!(
            map_support_to_category(Some(Public::Optional)),
            Category::Optional
        );
        assert_eq!(
            map_support_to_category(Some(Public::Conditional)),
            Category::Conditional
        );
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
                                data: Some(vec![2]),                 // Number of entries
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
        assert_eq!(map.get(&(0x1006, 0)), Some(&ObjectValue::Unsigned32(10000)));

        // Check RECORD
        assert_eq!(map.get(&(0x1018, 0)), Some(&ObjectValue::Unsigned8(2)));
        assert_eq!(
            map.get(&(0x1018, 1)),
            Some(&ObjectValue::Unsigned32(0x12345678))
        );
        assert_eq!(
            map.get(&(0x1018, 2)),
            Some(&ObjectValue::Unsigned32(0x00001234))
        );

        // Check ARRAY
        assert_eq!(map.get(&(0x2000, 0)), Some(&ObjectValue::Unsigned8(1)));
        assert_eq!(
            map.get(&(0x2000, 1)),
            Some(&ObjectValue::Unsigned16(0x2211))
        );
    }

    #[test]
    fn test_extract_nmt_settings() {
        let xdc_file = XdcFile {
            network_management: Some(NetworkManagement {
                general_features: GeneralFeatures {
                    dll_feature_mn: true, // MN
                    nmt_isochronous: Some(true), // ISOCHRONOUS
                    sdo_support_asnd: Some(true), // SDO_ASND
                    nmt_boot_time_not_active: 50000,
                    nmt_cycle_time_min: 100,
                    nmt_cycle_time_max: 50000,
                    ..Default::default()
                },
                mn_features: Some(MnFeatures {
                     nmt_service_udp_ip: Some(true), // NMT_SERVICE_UDP
                     ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };

        let settings = extract_nmt_settings(&xdc_file).unwrap();

        assert!(settings.feature_flags.contains(FeatureFlags::ISOCHRONOUS));
        assert!(settings.feature_flags.contains(FeatureFlags::SDO_ASND));
        assert!(settings.feature_flags.contains(FeatureFlags::NMT_SERVICE_UDP));
        assert!(!settings.feature_flags.contains(FeatureFlags::SDO_UDP)); // Should be false

        assert_eq!(settings.boot_time_not_active, 50000);
        assert_eq!(settings.cycle_time_min, 100);
        assert_eq!(settings.cycle_time_max, 50000);
    }

    #[test]
    fn test_extract_nmt_settings_missing_block() {
        let xdc_file = XdcFile::default(); // No network_management
        let result = extract_nmt_settings(&xdc_file);
        assert!(result.is_err());
    }
}