// crates/powerlink-rs-xdc/src/resolver/app_process.rs

//! Handles resolving the `ApplicationProcess` block from the model to public types.

use crate::error::XdcError;
use crate::model;
use crate::model::app_process::{AppDataTypeChoice, ParameterGroupItem};
use crate::model::common::{Glabels, LabelChoice};
// Removed unused import: use crate::parser::parse_hex_u8;
use crate::types;
use alloc::string::String;
use alloc::vec::Vec;
// Removed unused import: use crate::types::ParameterRef;

// --- Label Helpers ---

/// Helper to extract the first available `<label>` value from a `g_labels` group.
fn extract_label(labels: &Glabels) -> Option<String> {
    labels.items.iter().find_map(|item| {
        if let LabelChoice::Label(label) = item {
            Some(label.value.clone())
        } else {
            None
        }
    })
}

/// Helper to extract the first available `<description>` value from a `g_labels` group.
fn extract_description(labels: &Glabels) -> Option<String> {
    labels.items.iter().find_map(|item| {
        if let LabelChoice::Description(desc) = item {
            Some(desc.value.clone())
        } else {
            None
        }
    })
}

/// Helper to extract the name of a `ParameterDataType` enum.
fn get_data_type_name(data_type: &model::app_process::ParameterDataType) -> String {
    use model::app_process::ParameterDataType::*;
    match data_type {
        BOOL => "BOOL".into(),
        BITSTRING => "BITSTRING".into(),
        BYTE => "BYTE".into(),
        CHAR => "CHAR".into(),
        WORD => "WORD".into(),
        DWORD => "DWORD".into(),
        LWORD => "LWORD".into(),
        SINT => "SINT".into(),
        INT => "INT".into(),
        DINT => "DINT".into(),
        LINT => "LINT".into(),
        USINT => "USINT".into(),
        UINT => "UINT".into(),
        UDINT => "UDINT".into(),
        ULINT => "ULINT".into(),
        REAL => "REAL".into(),
        LREAL => "LREAL".into(),
        STRING => "STRING".into(),
        WSTRING => "WSTRING".into(),
        DataTypeIDRef(r) => r.unique_id_ref.clone(),
        VariableRef(_) => "variableRef".into(), // This case is complex, placeholder
    }
}

// --- Main Resolver ---

/// Resolves the `model::ApplicationProcess` into the public `types::ApplicationProcess`.
pub(super) fn resolve_application_process(
    model: &model::app_process::ApplicationProcess,
) -> Result<types::ApplicationProcess, XdcError> {
    let data_types = model
        .data_type_list
        .as_ref()
        .map_or(Ok(Vec::new()), resolve_data_type_list)?;

    let parameter_groups = model
        .parameter_group_list
        .as_ref()
        .map_or(Ok(Vec::new()), resolve_parameter_group_list)?;

    let function_types = model
        .function_type_list
        .as_ref()
        .map_or(Ok(Vec::new()), resolve_function_type_list)?;

    let function_instances = model
        .function_instance_list
        .as_ref()
        .map_or(Ok(Vec::new()), resolve_function_instance_list)?;

    Ok(types::ApplicationProcess {
        data_types,
        parameter_groups,
        function_types,
        function_instances,
    })
}

/// Helper to resolve the `<dataTypeList>`.
fn resolve_data_type_list(
    list: &model::app_process::AppDataTypeList,
) -> Result<Vec<types::AppDataType>, XdcError> {
    list.items
        .iter()
        .map(|item| match item {
            AppDataTypeChoice::Struct(s) => Ok(types::AppDataType::Struct(resolve_struct(s)?)),
            AppDataTypeChoice::Array(a) => Ok(types::AppDataType::Array(resolve_array(a)?)),
            AppDataTypeChoice::Enum(e) => Ok(types::AppDataType::Enum(resolve_enum(e)?)),
            AppDataTypeChoice::Derived(d) => Ok(types::AppDataType::Derived(resolve_derived(d)?)),
        })
        .collect()
}

/// Helper to resolve a `<struct>`.
fn resolve_struct(model: &model::app_process::AppStruct) -> Result<types::AppStruct, XdcError> {
    let members = model
        .var_declaration
        .iter()
        .map(|var| {
            // This is the fix: construct a types::StructMember directly
            Ok(types::StructMember {
                name: var.name.clone(),
                unique_id: var.unique_id.clone(),
                data_type: get_data_type_name(&var.data_type),
                size: var
                    .size
                    .as_ref()
                    .and_then(|s| s.parse::<u32>().ok()),
                label: extract_label(&var.labels),
                description: extract_description(&var.labels),
            })
        })
        .collect::<Result<Vec<_>, XdcError>>()?;

    Ok(types::AppStruct {
        name: model.name.clone(),
        unique_id: model.unique_id.clone(),
        label: extract_label(&model.labels),
        description: extract_description(&model.labels),
        members,
    })
}

/// Helper to resolve an `<array>`.
fn resolve_array(model: &model::app_process::AppArray) -> Result<types::AppArray, XdcError> {
    // XDC spec (7.4.7.2.3) implies single dimension for OD-style arrays,
    // but schema allows multiple. We'll take the first <subrange>.
    let subrange = model.subrange.first().ok_or(XdcError::MissingElement {
        element: "subrange",
    })?;

    Ok(types::AppArray {
        name: model.name.clone(),
        unique_id: model.unique_id.clone(),
        label: extract_label(&model.labels),
        description: extract_description(&model.labels),
        lower_limit: subrange.lower_limit.parse().unwrap_or(0),
        upper_limit: subrange.upper_limit.parse().unwrap_or(0),
        data_type: get_data_type_name(&model.data_type),
    })
}

/// Helper to resolve an `<enum>`.
fn resolve_enum(model: &model::app_process::AppEnum) -> Result<types::AppEnum, XdcError> {
    let values = model
        .enum_value
        .iter()
        .map(|val| types::EnumValue {
            name: extract_label(&val.labels).unwrap_or_default(), // Name comes from <label>
            value: val.value.clone().unwrap_or_default(),
            label: extract_label(&val.labels),
            description: extract_description(&val.labels),
        })
        .collect();

    Ok(types::AppEnum {
        name: model.name.clone(),
        unique_id: model.unique_id.clone(),
        label: extract_label(&model.labels),
        description: extract_description(&model.labels),
        data_type: model
            .data_type
            .as_ref()
            .map(get_data_type_name)
            .unwrap_or_default(),
        size_in_bits: model
            .size
            .as_ref()
            .and_then(|s| s.parse::<u32>().ok()),
        values,
    })
}

/// Helper to resolve a `<derived>` type.
fn resolve_derived(model: &model::app_process::AppDerived) -> Result<types::AppDerived, XdcError> {
    let count = model.count.as_ref().map(|c| types::Count {
        unique_id: c.unique_id.clone(),
        access: c.access.map(super::utils::map_param_access),
        default_value: Some(c.default_value.value.clone()),
    });

    Ok(types::AppDerived {
        name: model.name.clone(),
        unique_id: model.unique_id.clone(),
        label: extract_label(&model.labels),
        description: extract_description(&model.labels),
        data_type: get_data_type_name(&model.data_type),
        count,
    })
}

/// Helper to resolve the `<parameterGroupList>`.
fn resolve_parameter_group_list(
    list: &model::app_process::ParameterGroupList,
) -> Result<Vec<types::ParameterGroup>, XdcError> {
    list.parameter_group
        .iter()
        .map(resolve_parameter_group)
        .collect()
}

/// Helper to recursively resolve a `<parameterGroup>`.
fn resolve_parameter_group(
    model: &model::app_process::ParameterGroup,
) -> Result<types::ParameterGroup, XdcError> {
    let items = model
        .items
        .iter()
        .map(|item| match item {
            ParameterGroupItem::ParameterGroup(pg) => {
                Ok(types::ParameterGroupItem::Group(resolve_parameter_group(pg)?))
            }
            ParameterGroupItem::ParameterRef(pr) => {
                Ok(types::ParameterGroupItem::Parameter(types::ParameterRef {
                    unique_id_ref: pr.unique_id_ref.clone(),
                    visible: pr.visible,
                    locked: pr.locked,
                    bit_offset: pr
                        .bit_offset
                        .as_ref()
                        .and_then(|s| s.parse::<u32>().ok()),
                }))
            }
        })
        .collect::<Result<Vec<_>, XdcError>>()?;

    Ok(types::ParameterGroup {
        unique_id: model.unique_id.clone(),
        label: extract_label(&model.labels),
        description: extract_description(&model.labels),
        items,
    })
}

/// Helper to resolve `<functionTypeList>`.
fn resolve_function_type_list(
    list: &model::app_process::FunctionTypeList,
) -> Result<Vec<types::FunctionType>, XdcError> {
    list.function_type
        .iter()
        .map(resolve_function_type)
        .collect()
}

/// Helper to resolve a single `<functionType>`.
fn resolve_function_type(
    model: &model::app_process::FunctionType,
) -> Result<types::FunctionType, XdcError> {
    let version_info = model
        .version_info
        .iter()
        .map(|v| types::VersionInfo {
            organization: v.organization.clone(),
            version: v.version.clone(),
            author: v.author.clone(),
            date: v.date.clone(),
            label: extract_label(&v.labels),
            description: extract_description(&v.labels),
        })
        .collect();

    let interface = resolve_interface_list(&model.interface_list)?;

    // Note: We are not resolving the nested `functionInstanceList` inside a
    // `functionType` yet, as it's less critical for OD mapping.

    Ok(types::FunctionType {
        name: model.name.clone(),
        unique_id: model.unique_id.clone(),
        package: model.package.clone(),
        label: extract_label(&model.labels),
        description: extract_description(&model.labels),
        version_info,
        interface,
    })
}

/// Helper to resolve an `<interfaceList>`.
fn resolve_interface_list(
    model: &model::app_process::InterfaceList,
) -> Result<types::InterfaceList, XdcError> {
    let inputs = model
        .input_vars
        .as_ref()
        .map_or(Ok(Vec::new()), |vars| {
            vars.var_declaration
                .iter()
                .map(resolve_var_declaration)
                .collect()
        })?;

    let outputs = model
        .output_vars
        .as_ref()
        .map_or(Ok(Vec::new()), |vars| {
            vars.var_declaration
                .iter()
                .map(resolve_var_declaration)
                .collect()
        })?;

    let configs = model
        .config_vars
        .as_ref()
        .map_or(Ok(Vec::new()), |vars| {
            vars.var_declaration
                .iter()
                .map(resolve_var_declaration)
                .collect()
        })?;

    Ok(types::InterfaceList {
        inputs,
        outputs,
        configs,
    })
}

/// Helper to resolve a `<varDeclaration>`.
fn resolve_var_declaration(
    model: &model::app_process::VarDeclaration,
) -> Result<types::VarDeclaration, XdcError> {
    Ok(types::VarDeclaration {
        name: model.name.clone(),
        unique_id: model.unique_id.clone(),
        data_type: get_data_type_name(&model.data_type),
        size: model.size.as_ref().and_then(|s| s.parse::<u32>().ok()),
        initial_value: model.initial_value.clone(),
        label: extract_label(&model.labels),
        description: extract_description(&model.labels),
    })
}

/// Helper to resolve `<functionInstanceList>`.
fn resolve_function_instance_list(
    list: &model::app_process::FunctionInstanceList,
) -> Result<Vec<types::FunctionInstance>, XdcError> {
    list.function_instance
        .iter()
        .map(|inst| {
            Ok(types::FunctionInstance {
                name: inst.name.clone(),
                unique_id: inst.unique_id.clone(),
                type_id_ref: inst.type_id_ref.clone(),
                label: extract_label(&inst.labels),
                description: extract_description(&inst.labels),
            })
        })
        .collect()
    // Note: We are not resolving <connection> elements yet.
}