// crates/powerlink-rs-xdc/src/resolver/utils.rs

//! Utility functions for the resolver.

use crate::model;
use crate::model::app_layers::DataTypeName;
use crate::types;
use alloc::string::String;

/// Helper to extract the first available `<label>` value from a `g_labels` group.
pub(super) fn extract_label(items: &[model::common::LabelChoice]) -> Option<String> {
    items.iter().find_map(|item| {
        if let model::common::LabelChoice::Label(label) = item {
            Some(label.value.clone())
        } else {
            None
        }
    })
}

/// Helper to extract the first available `<description>` value from a `g_labels` group.
pub(super) fn extract_description(items: &[model::common::LabelChoice]) -> Option<String> {
    items.iter().find_map(|item| {
        if let model::common::LabelChoice::Description(desc) = item {
            Some(desc.value.clone())
        } else {
            None
        }
    })
}

/// Maps a POWERLINK hex string ID (from EPSG 311, Table 56) to the `DataTypeName` enum.
///
/// This centralizes the parsing of "0005" -> `Unsigned8`, etc.
pub(crate) fn get_standard_type_from_hex(type_id: &str) -> Option<DataTypeName> {
    // Strip optional "0x" prefix for robustness
    let id = type_id.strip_prefix("0x").unwrap_or(type_id);

    match id {
        "0001" => Some(DataTypeName::Boolean),
        "0002" => Some(DataTypeName::Integer8),
        "0003" => Some(DataTypeName::Integer16),
        "0004" => Some(DataTypeName::Integer32),
        "0005" => Some(DataTypeName::Unsigned8),
        "0006" => Some(DataTypeName::Unsigned16),
        "0007" => Some(DataTypeName::Unsigned32),
        "0008" => Some(DataTypeName::Real32),
        "0009" => Some(DataTypeName::VisibleString),
        "0010" => Some(DataTypeName::Integer24),
        "0011" => Some(DataTypeName::Real64),
        "0012" => Some(DataTypeName::Integer40),
        "0013" => Some(DataTypeName::Integer48),
        "0014" => Some(DataTypeName::Integer56),
        "0015" => Some(DataTypeName::Integer64),
        "000A" => Some(DataTypeName::OctetString),
        "000B" => Some(DataTypeName::UnicodeString),
        "000C" => Some(DataTypeName::TimeOfDay),
        "000D" => Some(DataTypeName::TimeDiff),
        "000F" => Some(DataTypeName::Domain),
        "0016" => Some(DataTypeName::Unsigned24),
        "0018" => Some(DataTypeName::Unsigned40),
        "0019" => Some(DataTypeName::Unsigned48),
        "001A" => Some(DataTypeName::Unsigned56),
        "001B" => Some(DataTypeName::Unsigned64),
        "0401" => Some(DataTypeName::MacAddress),
        "0402" => Some(DataTypeName::IpAddress),
        "0403" => Some(DataTypeName::NETTIME),
        _ => None,
    }
}

/// Maps the internal model enum (`ObjectAccessType`) to the public types enum (`ParameterAccess`).
pub(super) fn map_access_type(
    model: model::app_layers::ObjectAccessType,
) -> types::ParameterAccess {
    match model {
        model::app_layers::ObjectAccessType::ReadOnly => types::ParameterAccess::ReadOnly,
        model::app_layers::ObjectAccessType::WriteOnly => types::ParameterAccess::WriteOnly,
        model::app_layers::ObjectAccessType::ReadWrite => types::ParameterAccess::ReadWrite,
        model::app_layers::ObjectAccessType::Constant => types::ParameterAccess::Constant,
    }
}

/// Maps the internal model enum (`ObjectPdoMapping`) to the public types enum.
pub(super) fn map_pdo_mapping(
    model: model::app_layers::ObjectPdoMapping,
) -> types::ObjectPdoMapping {
    match model {
        model::app_layers::ObjectPdoMapping::No => types::ObjectPdoMapping::No,
        model::app_layers::ObjectPdoMapping::Default => types::ObjectPdoMapping::Default,
        model::app_layers::ObjectPdoMapping::Optional => types::ObjectPdoMapping::Optional,
        model::app_layers::ObjectPdoMapping::Tpdo => types::ObjectPdoMapping::Tpdo,
        model::app_layers::ObjectPdoMapping::Rpdo => types::ObjectPdoMapping::Rpdo,
    }
}

/// Maps the `ApplicationProcess` `ParameterAccess` enum to the public `types` enum.
pub(super) fn map_param_access(
    model: model::app_process::ParameterAccess,
) -> types::ParameterAccess {
    match model {
        model::app_process::ParameterAccess::Const => types::ParameterAccess::Constant,
        model::app_process::ParameterAccess::Read => types::ParameterAccess::ReadOnly,
        model::app_process::ParameterAccess::Write => types::ParameterAccess::WriteOnly,
        model::app_process::ParameterAccess::ReadWrite => types::ParameterAccess::ReadWrite,
        model::app_process::ParameterAccess::ReadWriteInput => {
            types::ParameterAccess::ReadWriteInput
        }
        model::app_process::ParameterAccess::ReadWriteOutput => {
            types::ParameterAccess::ReadWriteOutput
        }
        model::app_process::ParameterAccess::NoAccess => types::ParameterAccess::NoAccess,
    }
}

/// Maps the `ApplicationProcess` `ParameterSupport` enum to the public `types` enum.
pub(super) fn map_param_support(
    model: model::app_process::ParameterSupport,
) -> types::ParameterSupport {
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
    use crate::model::app_process::{
        ParameterAccess as ParamAccessModel, ParameterSupport as ParamSupportModel,
    };
    use crate::types::{
        ObjectPdoMapping as PdoPublic, ParameterAccess as ParamAccessPublic,
        ParameterSupport as ParamSupportPublic,
    };
    use alloc::collections::BTreeMap;
    use alloc::string::ToString;
    use alloc::vec;

    /// Helper to create a BTreeMap simulating a parsed <DataTypeList>
    fn get_test_type_map() -> BTreeMap<String, DataTypeName> {
        let mut map = BTreeMap::new();
        map.insert("0005".to_string(), DataTypeName::Unsigned8);
        map.insert("0006".to_string(), DataTypeName::Unsigned16);
        map.insert("0007".to_string(), DataTypeName::Unsigned32);
        map.insert("001B".to_string(), DataTypeName::Unsigned64);
        map
    }

    #[test]
    fn test_map_access_type() {
        assert_eq!(
            map_access_type(ObjectAccessType::ReadOnly),
            ParamAccessPublic::ReadOnly
        );
        assert_eq!(
            map_access_type(ObjectAccessType::WriteOnly),
            ParamAccessPublic::WriteOnly
        );
        assert_eq!(
            map_access_type(ObjectAccessType::ReadWrite),
            ParamAccessPublic::ReadWrite
        );
        assert_eq!(
            map_access_type(ObjectAccessType::Constant),
            ParamAccessPublic::Constant
        );
    }

    #[test]
    fn test_map_pdo_mapping() {
        assert_eq!(map_pdo_mapping(ObjectPdoMapping::No), PdoPublic::No);
        assert_eq!(
            map_pdo_mapping(ObjectPdoMapping::Default),
            PdoPublic::Default
        );
        assert_eq!(
            map_pdo_mapping(ObjectPdoMapping::Optional),
            PdoPublic::Optional
        );
        assert_eq!(map_pdo_mapping(ObjectPdoMapping::Tpdo), PdoPublic::Tpdo);
        assert_eq!(map_pdo_mapping(ObjectPdoMapping::Rpdo), PdoPublic::Rpdo);
    }

    #[test]
    fn test_map_param_access() {
        assert_eq!(
            map_param_access(ParamAccessModel::Const),
            ParamAccessPublic::Constant
        );
        assert_eq!(
            map_param_access(ParamAccessModel::Read),
            ParamAccessPublic::ReadOnly
        );
        assert_eq!(
            map_param_access(ParamAccessModel::Write),
            ParamAccessPublic::WriteOnly
        );
        assert_eq!(
            map_param_access(ParamAccessModel::ReadWrite),
            ParamAccessPublic::ReadWrite
        );
        assert_eq!(
            map_param_access(ParamAccessModel::ReadWriteInput),
            ParamAccessPublic::ReadWriteInput
        );
        assert_eq!(
            map_param_access(ParamAccessModel::ReadWriteOutput),
            ParamAccessPublic::ReadWriteOutput
        );
        assert_eq!(
            map_param_access(ParamAccessModel::NoAccess),
            ParamAccessPublic::NoAccess
        );
    }

    #[test]
    fn test_map_param_support() {
        assert_eq!(
            map_param_support(ParamSupportModel::Mandatory),
            ParamSupportPublic::Mandatory
        );
        assert_eq!(
            map_param_support(ParamSupportModel::Optional),
            ParamSupportPublic::Optional
        );
        assert_eq!(
            map_param_support(ParamSupportModel::Conditional),
            ParamSupportPublic::Conditional
        );
    }
}
