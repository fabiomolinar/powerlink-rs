// crates/powerlink-rs-xdc/src/resolver/utils.rs

//! Utility functions for the resolver.

use crate::error::XdcError;
use crate::model;
use crate::model::app_layers::DataTypeName;
use crate::types;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};

/// Helper to extract the first available `<label>` value from a `g_labels` group.
pub(super) fn extract_label(labels: &model::common::Glabels) -> Option<String> {
    labels.items.iter().find_map(|item| {
        if let model::common::LabelChoice::Label(label) = item {
            Some(label.value.clone())
        } else {
            None
        }
    })
}

/// Helper to extract the first available `<description>` value from a `g_labels` group.
pub(super) fn extract_description(labels: &model::common::Glabels) -> Option<String> {
    labels.items.iter().find_map(|item| {
        if let model::common::LabelChoice::Description(desc) = item {
            Some(desc.value.clone())
        } else {
            None
        }
    })
}

/// Validates that the length of the parsed data matches the expected
/// size of the given `dataType` ID.
pub(super) fn validate_type(
    index: u16,
    sub_index: u8,
    data: &[u8],
    data_type_id_str: &str,
    type_map: &BTreeMap<String, DataTypeName>,
) -> Result<(), XdcError> {
    if let Some(expected_len) = get_data_type_size(data_type_id_str, type_map) {
        if data.len() != expected_len {
            return Err(XdcError::TypeValidationError {
                index,
                sub_index,
                data_type: data_type_id_str.to_string(),
                expected_bytes: expected_len,
                actual_bytes: data.len(),
            });
        }
    }
    Ok(())
}

/// Maps a POWERLINK dataType ID (from EPSG 311, Table 56) to its expected byte size.
/// It first attempts to resolve the ID using the file-provided `type_map`.
/// If not found, it falls back to a hard-coded map.
/// Returns `None` for variable-sized types (like strings) or unknown types.
pub(super) fn get_data_type_size(
    type_id: &str,
    type_map: &BTreeMap<String, DataTypeName>,
) -> Option<usize> {
    if let Some(type_name) = type_map.get(type_id) {
        // --- Logic: Use the map from the XDD file's <DataTypeList> ---
        match type_name {
            DataTypeName::Boolean => Some(1),
            DataTypeName::Integer8 => Some(1),
            DataTypeName::Unsigned8 => Some(1),
            DataTypeName::Integer16 => Some(2),
            DataTypeName::Unsigned16 => Some(2),
            DataTypeName::Integer24 => Some(3),
            DataTypeName::Unsigned24 => Some(3),
            DataTypeName::Integer32 => Some(4),
            DataTypeName::Unsigned32 => Some(4),
            DataTypeName::Real32 => Some(4),
            DataTypeName::Integer40 => Some(5),
            DataTypeName::Unsigned40 => Some(5),
            DataTypeName::Integer48 => Some(6),
            DataTypeName::Unsigned48 => Some(6),
            DataTypeName::Integer56 => Some(7),
            DataTypeName::Unsigned56 => Some(7),
            DataTypeName::Integer64 => Some(8),
            DataTypeName::Unsigned64 => Some(8),
            DataTypeName::Real64 => Some(8),
            DataTypeName::MacAddress => Some(6),
            DataTypeName::IpAddress => Some(4),
            DataTypeName::NETTIME => Some(8),
            // Variable-sized types:
            DataTypeName::VisibleString
            | DataTypeName::OctetString
            | DataTypeName::UnicodeString
            | DataTypeName::TimeOfDay
            | DataTypeName::TimeDiff
            | DataTypeName::Domain => None,
        }
    } else {
        // --- Fallback Logic: Use hard-coded hex IDs per EPSG 311, Table 56 ---
        // This block is now corrected.
        match type_id {
            "0001" => Some(1), // Boolean
            "0002" => Some(1), // Integer8
            "0003" => Some(2), // Integer16
            "0004" => Some(4), // Integer32
            "0005" => Some(1), // Unsigned8
            "0006" => Some(2), // Integer16
            "0007" => Some(4), // Unsigned32
            "0008" => Some(4), // Real32
            "0010" => Some(3), // Integer24
            "0011" => Some(8), // Real64
            "0012" => Some(5), // Integer40
            "0013" => Some(6), // Integer48 - Corrected
            "0014" => Some(7), // Integer56
            "0015" => Some(8), // Integer64
            "0016" => Some(3), // Unsigned24
            "0018" => Some(5), // Unsigned40 - Corrected
            "0019" => Some(6), // Unsigned48 - Corrected
            "001A" => Some(7), // Unsigned56 - Corrected
            "001B" => Some(8), // Unsigned64
            "0401" => Some(6), // MAC_ADDRESS
            "0402" => Some(4), // IP_ADDRESS
            "0403" => Some(8), // NETTIME
            // Variable-sized types (return None):
            "0009" // Visible_String
            | "000A" // Octet_String
            | "000B" // Unicode_String
            | "000C" // Time_of_Day
            | "000D" // Time_Diff
            | "000F" // Domain
            // | "001A" // BITSTRING - This is listed as Unsigned56 in Table 56
            => None,
            // Unknown types:
            _ => None,
        }
    }
}

/// Maps the internal model enum (`ObjectAccessType`) to the public types enum (`ParameterAccess`).
pub(super) fn map_access_type(model: model::app_layers::ObjectAccessType) -> types::ParameterAccess {
    match model {
        model::app_layers::ObjectAccessType::ReadOnly => types::ParameterAccess::ReadOnly,
        model::app_layers::ObjectAccessType::WriteOnly => types::ParameterAccess::WriteOnly,
        model::app_layers::ObjectAccessType::ReadWrite => types::ParameterAccess::ReadWrite,
        model::app_layers::ObjectAccessType::Constant => types::ParameterAccess::Constant,
    }
}

/// Maps the internal model enum (`ObjectPdoMapping`) to the public types enum.
pub(super) fn map_pdo_mapping(model: model::app_layers::ObjectPdoMapping) -> types::ObjectPdoMapping {
    match model {
        model::app_layers::ObjectPdoMapping::No => types::ObjectPdoMapping::No,
        model::app_layers::ObjectPdoMapping::Default => types::ObjectPdoMapping::Default,
        model::app_layers::ObjectPdoMapping::Optional => types::ObjectPdoMapping::Optional,
        model::app_layers::ObjectPdoMapping::Tpdo => types::ObjectPdoMapping::Tpdo,
        model::app_layers::ObjectPdoMapping::Rpdo => types::ObjectPdoMapping::Rpdo,
    }
}

/// Maps the `ApplicationProcess` `ParameterAccess` enum to the public `types` enum.
pub(super) fn map_param_access(model: model::app_process::ParameterAccess) -> types::ParameterAccess {
    match model {
        model::app_process::ParameterAccess::Const => types::ParameterAccess::Constant,
        model::app_process::ParameterAccess::Read => types::ParameterAccess::ReadOnly,
        model::app_process::ParameterAccess::Write => types::ParameterAccess::WriteOnly,
        model::app_process::ParameterAccess::ReadWrite => types::ParameterAccess::ReadWrite,
        model::app_process::ParameterAccess::ReadWriteInput => types::ParameterAccess::ReadWriteInput,
        model::app_process::ParameterAccess::ReadWriteOutput => types::ParameterAccess::ReadWriteOutput,
        model::app_process::ParameterAccess::NoAccess => types::ParameterAccess::NoAccess,
    }
}

/// Maps the `ApplicationProcess` `ParameterSupport` enum to the public `types` enum.
pub(super) fn map_param_support(model: model::app_process::ParameterSupport) -> types::ParameterSupport {
    match model {
        model::app_process::ParameterSupport::Mandatory => types::ParameterSupport::Mandatory,
        model::app_process::ParameterSupport::Optional => types::ParameterSupport::Optional,
        model::app_process::ParameterSupport::Conditional => types::ParameterSupport::Conditional,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::app_layers::{DataTypeName, ObjectAccessType, ObjectPdoMapping};
    use crate::model::app_process::{ParameterAccess as ParamAccessModel, ParameterSupport as ParamSupportModel};
    use crate::types::{ParameterAccess as ParamAccessPublic, ParameterSupport as ParamSupportPublic, ObjectPdoMapping as PdoPublic};
    use alloc::collections::BTreeMap;
    use alloc::string::ToString;
    use alloc::vec;

    /// Helper to create a BTreeMap simulating a parsed <DataTypeList>
    fn get_test_type_map() -> BTreeMap<String, DataTypeName> {
        let mut map = BTreeMap::new();
        map.insert("0005".to_string(), DataTypeName::Unsigned8);
        map.insert("0007".to_string(), DataTypeName::Unsigned32);
        map.insert("0009".to_string(), DataTypeName::VisibleString);
        map.insert("0401".to_string(), DataTypeName::MacAddress);
        map
    }

    #[test]
    fn test_get_data_type_size_from_type_map() {
        let type_map = get_test_type_map();
        // Test types present in the map
        assert_eq!(get_data_type_size("0005", &type_map), Some(1));
        assert_eq!(get_data_type_size("0007", &type_map), Some(4));
        assert_eq!(get_data_type_size("0401", &type_map), Some(6));
        // Test variable-sized type
        assert_eq!(get_data_type_size("0009", &type_map), None);
    }

    #[test]
    fn test_get_data_type_size_from_fallback() {
        let empty_map = BTreeMap::new();
        // Test types not in the map but in the fallback
        assert_eq!(get_data_type_size("0003", &empty_map), Some(2)); // Integer16
        assert_eq!(get_data_type_size("0010", &empty_map), Some(3)); // Integer24
        assert_eq!(get_data_type_size("001B", &empty_map), Some(8)); // Unsigned64
        assert_eq!(get_data_type_size("0403", &empty_map), Some(8)); // NETTIME
        
        // Test variable-sized type
        assert_eq!(get_data_type_size("000A", &empty_map), None); // Octet_String
        // Test unknown type
        assert_eq!(get_data_type_size("FFFF", &empty_map), None);
    }

    #[test]
    fn test_validate_type() {
        let type_map = get_test_type_map();
        
        // 1. Success case
        let data_ok = vec![0x12, 0x34, 0x56, 0x78];
        let result_ok = validate_type(0x1000, 1, &data_ok, "0007", &type_map);
        assert!(result_ok.is_ok());

        // 2. Failure case (length mismatch)
        let data_fail = vec![0x12, 0x34]; // 2 bytes
        let result_fail = validate_type(0x1000, 1, &data_fail, "0007", &type_map); // Expects 4 bytes
        assert!(matches!(result_fail, Err(XdcError::TypeValidationError {
            index: 0x1000,
            sub_index: 1,
            data_type: _,
            expected_bytes: 4,
            actual_bytes: 2,
        })));

        // 3. Success on variable-sized type (should always pass)
        let data_var = vec![0x48, 0x69];
        let result_var = validate_type(0x1008, 0, &data_var, "0009", &type_map);
        assert!(result_var.is_ok());
    }

    #[test]
    fn test_map_access_type() {
        assert_eq!(map_access_type(ObjectAccessType::ReadOnly), ParamAccessPublic::ReadOnly);
        assert_eq!(map_access_type(ObjectAccessType::WriteOnly), ParamAccessPublic::WriteOnly);
        assert_eq!(map_access_type(ObjectAccessType::ReadWrite), ParamAccessPublic::ReadWrite);
        assert_eq!(map_access_type(ObjectAccessType::Constant), ParamAccessPublic::Constant);
    }

    #[test]
    fn test_map_pdo_mapping() {
        assert_eq!(map_pdo_mapping(ObjectPdoMapping::No), PdoPublic::No);
        assert_eq!(map_pdo_mapping(ObjectPdoMapping::Default), PdoPublic::Default);
        assert_eq!(map_pdo_mapping(ObjectPdoMapping::Optional), PdoPublic::Optional);
        assert_eq!(map_pdo_mapping(ObjectPdoMapping::Tpdo), PdoPublic::Tpdo);
        assert_eq!(map_pdo_mapping(ObjectPdoMapping::Rpdo), PdoPublic::Rpdo);
    }

    #[test]
    fn test_map_param_access() {
        assert_eq!(map_param_access(ParamAccessModel::Const), ParamAccessPublic::Constant);
        assert_eq!(map_param_access(ParamAccessModel::Read), ParamAccessPublic::ReadOnly);
        assert_eq!(map_param_access(ParamAccessModel::Write), ParamAccessPublic::WriteOnly);
        assert_eq!(map_param_access(ParamAccessModel::ReadWrite), ParamAccessPublic::ReadWrite);
        assert_eq!(map_param_access(ParamAccessModel::ReadWriteInput), ParamAccessPublic::ReadWriteInput);
        assert_eq!(map_param_access(ParamAccessModel::ReadWriteOutput), ParamAccessPublic::ReadWriteOutput);
        assert_eq!(map_param_access(ParamAccessModel::NoAccess), ParamAccessPublic::NoAccess);
    }

    #[test]
    fn test_map_param_support() {
        assert_eq!(map_param_support(ParamSupportModel::Mandatory), ParamSupportPublic::Mandatory);
        assert_eq!(map_param_support(ParamSupportModel::Optional), ParamSupportPublic::Optional);
        assert_eq!(map_param_support(ParamSupportModel::Conditional), ParamSupportPublic::Conditional);
    }
}