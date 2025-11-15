// crates/powerlink-rs-xdc/src/resolver/utils.rs

//! Utility functions for the resolver.

use crate::error::XdcError;
use crate::model;
use crate::model::app_layers::DataTypeName;
use crate::types;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};

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
            "0006" => Some(2), // Unsigned16
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