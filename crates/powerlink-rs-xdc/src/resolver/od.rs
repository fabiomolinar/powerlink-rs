// crates/powerlink-rs-xdc/src/resolver/od.rs

use crate::error::XdcError;
use crate::model;
use crate::model::app_layers::DataTypeName;
use crate::parser::{parse_hex_u16, parse_hex_u8, parse_hex_string};
use crate::resolver::{utils, ValueMode};
use crate::types;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Iterates the `model::ObjectList` and resolves it into a rich, public `types::ObjectDictionary`.
pub(super) fn resolve_object_dictionary<'a>(
    app_layers: &'a model::app_layers::ApplicationLayers,
    param_map: &'a BTreeMap<&'a String, &'a model::app_process::Parameter>,
    template_map: &'a BTreeMap<&'a String, &'a model::app_process::Value>,
    type_map: &BTreeMap<String, DataTypeName>,
    mode: ValueMode,
) -> Result<types::ObjectDictionary, XdcError> {
    let mut od_objects = Vec::new();

    for model_obj in &app_layers.object_list.object {
        let index = parse_hex_u16(&model_obj.index)?;

        // --- Start: Resolve Object Attributes (Task 9) ---
        // Set defaults from the <Object> tag itself
        let mut resolved_access = model_obj.access_type.map(utils::map_access_type);
        let mut resolved_support = None;
        let mut resolved_persistent = false;
        // MODIFIED: Add allowed_values
        let mut resolved_allowed_values: Option<types::AllowedValues> = None;
        let mut object_data: Option<Vec<u8>> = None;
        let mut od_sub_objects: Vec<types::SubObject> = Vec::new();

        // Check if a parameter reference overrides these attributes
        if let Some(id_ref) = model_obj.unique_id_ref.as_ref() {
            if let Some(param) = param_map.get(id_ref) {
                // The parameter's attributes take precedence.
                resolved_access = param.access.map(utils::map_param_access);
                resolved_support = param.support.map(utils::map_param_support);
                resolved_persistent = param.persistent;
                // MODIFIED: Resolve allowedValues from the parameter
                resolved_allowed_values = param
                    .allowed_values
                    .as_ref()
                    .map(resolve_allowed_values)
                    .transpose()?;
            }
        }
        // --- End: Resolve Object Attributes ---

        if model_obj.object_type == "7" {
            // This is a VAR. Its value is on the <Object> element itself.
            let value_str_opt = get_value_str_for_object(model_obj, mode, param_map, template_map);

            // We only store data if it's valid hex.
            object_data = value_str_opt.and_then(|s| parse_hex_string(s).ok());

            // Perform type validation if we have data
            if let (Some(data), Some(data_type_id)) =
                (object_data.as_ref(), model_obj.data_type.as_deref())
            {
                utils::validate_type(index, 0, data, data_type_id, type_map)?;
            }
        } else {
            // This is a RECORD or ARRAY. Process its <SubObject> children.
            for model_sub_obj in &model_obj.sub_object {
                let sub_index = parse_hex_u8(&model_sub_obj.sub_index)?;

                // --- Start: Resolve SubObject Attributes (Task 9) ---
                // Set defaults from the <SubObject> tag itself
                let mut sub_resolved_access = model_sub_obj.access_type.map(utils::map_access_type);
                let mut sub_resolved_support = None;
                let mut sub_resolved_persistent = false;
                // MODIFIED: Add allowed_values
                let mut sub_resolved_allowed_values: Option<types::AllowedValues> = None;

                // Check if a parameter reference overrides these attributes
                if let Some(id_ref) = model_sub_obj.unique_id_ref.as_ref() {
                    if let Some(param) = param_map.get(id_ref) {
                        // The parameter's attributes take precedence.
                        sub_resolved_access = param.access.map(utils::map_param_access);
                        sub_resolved_support = param.support.map(utils::map_param_support);
                        sub_resolved_persistent = param.persistent;
                        // MODIFIED: Resolve allowedValues from the parameter
                        sub_resolved_allowed_values = param
                            .allowed_values
                            .as_ref()
                            .map(resolve_allowed_values)
                            .transpose()?;
                    }
                }
                // --- End: Resolve SubObject Attributes ---

                // Logic to find the correct value string
                let value_str_opt = get_value_str_for_subobject(
                    model_sub_obj,
                    mode,
                    param_map,
                    template_map,
                    model_obj.unique_id_ref.as_ref(),
                    sub_index,
                );

                // We only store data if it's valid hex.
                // Non-hex values (like "NumberOfEntries") result in `None`.
                let data = value_str_opt.and_then(|s| parse_hex_string(s).ok());

                // Perform type validation if we have data
                if let (Some(data), Some(data_type_id)) = (
                    data.as_ref(),
                    model_sub_obj.data_type.as_deref(),
                ) {
                    utils::validate_type(index, sub_index, data, data_type_id, type_map)?;
                }
                
                let pdo_mapping = model_sub_obj.pdo_mapping.map(utils::map_pdo_mapping);

                od_sub_objects.push(types::SubObject {
                    sub_index,
                    name: model_sub_obj.name.clone(),
                    object_type: model_sub_obj.object_type.clone(),
                    data_type: model_sub_obj.data_type.clone(),
                    low_limit: model_sub_obj.low_limit.clone(),
                    high_limit: model_sub_obj.high_limit.clone(),
                    access_type: sub_resolved_access, // Use resolved value
                    pdo_mapping,
                    obj_flags: model_sub_obj.obj_flags.clone(),
                    support: sub_resolved_support,   // Use resolved value
                    persistent: sub_resolved_persistent, // Use resolved value
                    allowed_values: sub_resolved_allowed_values, // MODIFIED: Assign resolved values
                    data,
                });
            }
        }
        
        let pdo_mapping = model_obj.pdo_mapping.map(utils::map_pdo_mapping);

        od_objects.push(types::Object {
            index,
            name: model_obj.name.clone(),
            object_type: model_obj.object_type.clone(),
            data_type: model_obj.data_type.clone(),
            low_limit: model_obj.low_limit.clone(), // MODIFIED: Pass through low_limit
            high_limit: model_obj.high_limit.clone(), // MODIFIED: Pass through high_limit
            access_type: resolved_access, // Use resolved value
            pdo_mapping,
            obj_flags: model_obj.obj_flags.clone(),
            support: resolved_support,   // Use resolved value
            persistent: resolved_persistent, // Use resolved value
            allowed_values: resolved_allowed_values, // MODIFIED: Assign resolved values
            data: object_data,
            sub_objects: od_sub_objects,
        });
    }

    Ok(types::ObjectDictionary {
        objects: od_objects,
    })
}

// --- NEW Helper to resolve allowedValues ---
/// Resolves a `model::app_process::AllowedValues` into a `types::AllowedValues`.
fn resolve_allowed_values(
    model: &model::app_process::AllowedValues,
) -> Result<types::AllowedValues, XdcError> {
    let values = model
        .value
        .iter()
        .map(|v| types::Value {
            value: v.value.clone(),
            label: v.labels.as_ref().and_then(utils::extract_label),
        })
        .collect();

    let ranges = model
        .range
        .iter()
        .map(|r| types::ValueRange {
            min_value: r.min_value.value.clone(),
            max_value: r.max_value.value.clone(),
            step: r.step.as_ref().map(|s| s.value.clone()),
        })
        .collect();

    Ok(types::AllowedValues { values, ranges })
}

/// Resolves the value string for an Object or Parameter.
/// (Helper for get_value_str_... functions)
fn resolve_value_from_param<'a>(
    param: &'a model::app_process::Parameter,
    mode: ValueMode,
    template_map: &'a BTreeMap<&'a String, &'a model::app_process::Value>,
) -> Option<&'a String> {
    // 1. Check for a direct value on the parameter
    let direct_value = match mode {
        ValueMode::Actual => param.actual_value.as_ref().or(param.default_value.as_ref()),
        ValueMode::Default => param.default_value.as_ref().or(param.actual_value.as_ref()),
    };
    
    direct_value
        .map(|v| &v.value)
        .or_else(|| {
            // 2. If no direct value, check for a template reference
            param
                .template_id_ref
                .as_ref()
                .and_then(|template_id| template_map.get(template_id))
                .map(|v| &v.value)
        })
}

/// Helper to get the raw value string for a VAR object.
fn get_value_str_for_object<'a>(
    model_obj: &'a model::app_layers::Object,
    mode: ValueMode,
    param_map: &'a BTreeMap<&'a String, &'a model::app_process::Parameter>,
    template_map: &'a BTreeMap<&'a String, &'a model::app_process::Value>,
) -> Option<&'a String> {
    // 1. Check for direct value on the <Object> tag
    let direct_value = match mode {
        ValueMode::Actual => model_obj.actual_value.as_ref().or(model_obj.default_value.as_ref()),
        ValueMode::Default => model_obj.default_value.as_ref().or(model_obj.actual_value.as_ref()),
    };

    direct_value.or_else(|| {
        // 2. If no direct value, resolve via uniqueIDRef
        model_obj
            .unique_id_ref
            .as_ref()
            .and_then(|id_ref| param_map.get(id_ref))
            .and_then(|param| resolve_value_from_param(param, mode, template_map))
    })
}

/// Helper to get the raw value string for a SubObject.
fn get_value_str_for_subobject<'a>(
    model_sub_obj: &'a model::app_layers::SubObject,
    mode: ValueMode,
    param_map: &'a BTreeMap<&'a String, &'a model::app_process::Parameter>,
    template_map: &'a BTreeMap<&'a String, &'a model::app_process::Value>,
    parent_unique_id_ref: Option<&'a String>,
    sub_index: u8,
) -> Option<&'a String> {
    // 1. Check for direct value on the <SubObject> tag
    let direct_value = match mode {
        ValueMode::Actual => model_sub_obj.actual_value.as_ref().or(model_sub_obj.default_value.as_ref()),
        ValueMode::Default => model_sub_obj.default_value.as_ref().or(model_sub_obj.actual_value.as_ref()),
    };
    
    direct_value
        .or_else(|| {
            // 2. If no direct value, resolve via SubObject's uniqueIDRef
            model_sub_obj
                .unique_id_ref
                .as_ref()
                .and_then(|id_ref| param_map.get(id_ref))
                .and_then(|param| resolve_value_from_param(param, mode, template_map))
        })
        .or_else(|| {
            // 3. If still None, and we are sub-index 0, check the parent Object's uniqueIDRef
            if sub_index == 0 {
                parent_unique_id_ref
                    .and_then(|id_ref| param_map.get(id_ref))
                    .and_then(|param| resolve_value_from_param(param, mode, template_map))
            } else {
                None
            }
        })
}