//! Resolves the Object Dictionary (OD) from the Application Layers model.
//!
//! This module handles the complex resolution logic for OD Objects. In XDC files,
//! the value of an object might be defined in several places:
//! 1.  Directly on the `<Object>` or `<SubObject>` element (as `actualValue` or `defaultValue`).
//! 2.  Indirectly via a `uniqueIDRef` pointing to a `<Parameter>` in the `ApplicationProcess`.
//! 3.  Further indirectly via that `<Parameter>` pointing to a `<ParameterTemplate>`.
//!
//! This module flattens this hierarchy into a simple `ObjectDictionary` struct.

use super::{utils, ValueMode};
use crate::error::XdcError;
use crate::model;
use crate::model::app_layers::DataTypeName;
use crate::parser::{parse_hex_u8, parse_hex_u16};
use crate::types;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Intermediate structure to hold attributes resolved from multiple sources.
struct ResolvedAttributes {
    access: Option<types::ParameterAccess>,
    support: Option<types::ParameterSupport>,
    persistent: bool,
    allowed_values: Option<types::AllowedValues>,
    /// The resolved value string (if any).
    data: Option<String>,
    data_type: Option<String>,
}

/// Resolves the `ObjectDictionary` from the raw model.
///
/// # Arguments
///
/// * `app_layers` - The raw ApplicationLayers model.
/// * `param_map` - Map of uniqueIDs to Parameters (from Pass 2).
/// * `template_map` - Map of uniqueIDs to Templates (from Pass 1).
/// * `mode` - Whether to prioritize actual or default values.
pub(super) fn resolve_object_dictionary<'a>(
    app_layers: &'a model::app_layers::ApplicationLayers,
    param_map: &'a BTreeMap<&'a String, &'a model::app_process::Parameter>,
    template_map: &'a BTreeMap<&'a String, &'a model::app_process::Value>,
    _type_map: &BTreeMap<String, DataTypeName>,
    mode: ValueMode,
) -> Result<types::ObjectDictionary, XdcError> {
    let mut od_objects = Vec::new();

    for model_obj in &app_layers.object_list.object {
        let index = parse_hex_u16(&model_obj.index)?;

        // Resolve attributes for the main object
        let attributes = resolve_object_attributes(model_obj, param_map, template_map, mode)?;

        // Resolve attributes for all sub-objects
        let sub_objects = resolve_sub_objects(
            &model_obj.sub_object,
            model_obj.unique_id_ref.as_ref(),
            param_map,
            template_map,
            mode,
        )?;

        let pdo_mapping = model_obj.pdo_mapping.map(utils::map_pdo_mapping);

        od_objects.push(types::Object {
            index,
            name: model_obj.name.clone(),
            object_type: model_obj.object_type.clone(),
            data_type: attributes.data_type,
            low_limit: model_obj.low_limit.clone(),
            high_limit: model_obj.high_limit.clone(),
            access_type: attributes.access,
            pdo_mapping,
            obj_flags: model_obj.obj_flags.clone(),
            support: attributes.support,
            persistent: attributes.persistent,
            allowed_values: attributes.allowed_values,
            data: attributes.data,
            sub_objects,
        });
    }

    Ok(types::ObjectDictionary {
        objects: od_objects,
    })
}

/// Resolves attributes for a single Object, walking the reference chain if necessary.
fn resolve_object_attributes<'a>(
    model_obj: &'a model::app_layers::Object,
    param_map: &'a BTreeMap<&'a String, &'a model::app_process::Parameter>,
    template_map: &'a BTreeMap<&'a String, &'a model::app_process::Value>,
    mode: ValueMode,
) -> Result<ResolvedAttributes, XdcError> {
    let mut resolved = ResolvedAttributes {
        access: model_obj.access_type.map(utils::map_access_type),
        support: None,
        persistent: false,
        allowed_values: None,
        data: None,
        data_type: model_obj.data_type.clone(),
    };

    // If the Object has a uniqueIDRef, it inherits properties from the Application Process Parameter.
    if let Some(id_ref) = model_obj.unique_id_ref.as_ref() {
        if let Some(param) = param_map.get(id_ref) {
            apply_parameter_attributes(param, &mut resolved)?;
            // If the OD definition didn't specify a DataType, inherit it from the Parameter.
            if resolved.data_type.is_none() {
                resolved.data_type = map_param_type_to_hex(&param.data_type);
            }
        }
    }

    // Resolve the value string.
    // Note: We only resolve values for VAR (type 7) objects here.
    // ARRAY and RECORD objects hold their values in their SubObjects.
    if model_obj.object_type == "7" {
        let value_str_opt = get_value_str_for_object(model_obj, mode, param_map, template_map);
        resolved.data = value_str_opt.cloned();
    }

    Ok(resolved)
}

/// Resolves a list of sub-objects.
fn resolve_sub_objects<'a>(
    model_subs: &'a [model::app_layers::SubObject],
    parent_unique_id_ref: Option<&'a String>,
    param_map: &'a BTreeMap<&'a String, &'a model::app_process::Parameter>,
    template_map: &'a BTreeMap<&'a String, &'a model::app_process::Value>,
    mode: ValueMode,
) -> Result<Vec<types::SubObject>, XdcError> {
    let mut result = Vec::new();

    for model_sub in model_subs {
        let sub_index = parse_hex_u8(&model_sub.sub_index)?;

        let mut resolved = ResolvedAttributes {
            access: model_sub.access_type.map(utils::map_access_type),
            support: None,
            persistent: false,
            allowed_values: None,
            data: None,
            data_type: model_sub.data_type.clone(),
        };

        // SubObjects can also have uniqueIDRefs pointing to specific Parameters.
        if let Some(id_ref) = model_sub.unique_id_ref.as_ref() {
            if let Some(param) = param_map.get(id_ref) {
                apply_parameter_attributes(param, &mut resolved)?;
                if resolved.data_type.is_none() {
                    resolved.data_type = map_param_type_to_hex(&param.data_type);
                }
            }
        }

        // Resolve the value.
        // Sub-index 0 is special: if the Parent Object has a uniqueIDRef, it might point
        // to a Parameter that defines the 'count' or array size, which maps to SubIndex 0.
        let value_str_opt = get_value_str_for_subobject(
            model_sub,
            mode,
            param_map,
            template_map,
            parent_unique_id_ref,
            sub_index,
        );

        resolved.data = value_str_opt.cloned();

        let pdo_mapping = model_sub.pdo_mapping.map(utils::map_pdo_mapping);

        result.push(types::SubObject {
            sub_index,
            name: model_sub.name.clone(),
            object_type: model_sub.object_type.clone(),
            data_type: resolved.data_type,
            low_limit: model_sub.low_limit.clone(),
            high_limit: model_sub.high_limit.clone(),
            access_type: resolved.access,
            pdo_mapping,
            obj_flags: model_sub.obj_flags.clone(),
            support: resolved.support,
            persistent: resolved.persistent,
            allowed_values: resolved.allowed_values,
            data: resolved.data,
        });
    }

    Ok(result)
}

/// Applies attributes from an Application Process `Parameter` to the resolved attributes.
fn apply_parameter_attributes(
    param: &model::app_process::Parameter,
    resolved: &mut ResolvedAttributes,
) -> Result<(), XdcError> {
    if let Some(acc) = param.access {
        resolved.access = Some(utils::map_param_access(acc));
    }
    if let Some(supp) = param.support {
        resolved.support = Some(utils::map_param_support(supp));
    }
    resolved.persistent = param.persistent;

    if let Some(av) = &param.allowed_values {
        resolved.allowed_values = Some(resolve_allowed_values(av)?);
    }
    Ok(())
}

/// Maps Application Process data types to standard POWERLINK hex codes.
fn map_param_type_to_hex(dt: &model::app_process::ParameterDataType) -> Option<String> {
    use model::app_process::ParameterDataType as PDT;
    match dt {
        PDT::BOOL => Some("0001".to_string()),
        PDT::SINT => Some("0002".to_string()),
        PDT::INT => Some("0003".to_string()),
        PDT::DINT => Some("0004".to_string()),
        PDT::USINT | PDT::BYTE => Some("0005".to_string()),
        PDT::UINT | PDT::WORD => Some("0006".to_string()),
        PDT::UDINT | PDT::DWORD => Some("0007".to_string()),
        PDT::REAL => Some("0008".to_string()),
        PDT::STRING => Some("0009".to_string()),
        PDT::LINT => Some("0015".to_string()),
        PDT::ULINT | PDT::LWORD => Some("001B".to_string()),
        PDT::LREAL => Some("0011".to_string()),
        PDT::WSTRING => Some("000B".to_string()),
        // Reference types (DataTypeIDRef, VariableRef) cannot be simply mapped to a hex code
        // without further context, so we return None.
        _ => None,
    }
}

fn resolve_allowed_values(
    model: &model::app_process::AllowedValues,
) -> Result<types::AllowedValues, XdcError> {
    let values = model
        .value
        .iter()
        .map(|v| types::Value {
            value: v.value.clone(),
            label: v
                .labels
                .as_ref()
                .and_then(|glabels| utils::extract_label(&glabels.items)),
            offset: v.offset.clone(),
            multiplier: v.multiplier.clone(),
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

    Ok(types::AllowedValues {
        template_id_ref: model.template_id_ref.clone(),
        values,
        ranges,
    })
}

/// Resolves the value string from a Parameter, respecting precedence rules.
///
/// Precedence:
/// 1. Direct value in the Parameter (Actual vs Default based on mode).
/// 2. Value from the referenced Template.
fn resolve_value_from_param<'a>(
    param: &'a model::app_process::Parameter,
    mode: ValueMode,
    template_map: &'a BTreeMap<&'a String, &'a model::app_process::Value>,
) -> Option<&'a String> {
    // 1. Check direct value in parameter
    let direct_value = match mode {
        ValueMode::Actual => param.actual_value.as_ref().or(param.default_value.as_ref()),
        ValueMode::Default => param.default_value.as_ref().or(param.actual_value.as_ref()),
    };

    // 2. Check template reference if no direct value found
    direct_value.map(|v| &v.value).or_else(|| {
        param
            .template_id_ref
            .as_ref()
            .and_then(|template_id| template_map.get(template_id))
            .map(|v| &v.value)
    })
}

/// Determines the string value for a top-level Object.
///
/// Precedence:
/// 1. Direct attribute on `<Object>` (Actual vs Default).
/// 2. Value derived from `uniqueIDRef` Parameter.
fn get_value_str_for_object<'a>(
    model_obj: &'a model::app_layers::Object,
    mode: ValueMode,
    param_map: &'a BTreeMap<&'a String, &'a model::app_process::Parameter>,
    template_map: &'a BTreeMap<&'a String, &'a model::app_process::Value>,
) -> Option<&'a String> {
    let direct_value = match mode {
        ValueMode::Actual => model_obj
            .actual_value
            .as_ref()
            .or(model_obj.default_value.as_ref()),
        ValueMode::Default => model_obj
            .default_value
            .as_ref()
            .or(model_obj.actual_value.as_ref()),
    };

    direct_value.or_else(|| {
        model_obj
            .unique_id_ref
            .as_ref()
            .and_then(|id_ref| param_map.get(id_ref))
            .and_then(|param| resolve_value_from_param(param, mode, template_map))
    })
}

/// Determines the string value for a SubObject.
///
/// Precedence:
/// 1. Direct attribute on `<SubObject>`.
/// 2. Value derived from `uniqueIDRef` on the SubObject.
/// 3. (If SubIndex == 0) Value derived from the parent Object's `uniqueIDRef` (implicit array/record size).
fn get_value_str_for_subobject<'a>(
    model_sub_obj: &'a model::app_layers::SubObject,
    mode: ValueMode,
    param_map: &'a BTreeMap<&'a String, &'a model::app_process::Parameter>,
    template_map: &'a BTreeMap<&'a String, &'a model::app_process::Value>,
    parent_unique_id_ref: Option<&'a String>,
    sub_index: u8,
) -> Option<&'a String> {
    let direct_value = match mode {
        ValueMode::Actual => model_sub_obj
            .actual_value
            .as_ref()
            .or(model_sub_obj.default_value.as_ref()),
        ValueMode::Default => model_sub_obj
            .default_value
            .as_ref()
            .or(model_sub_obj.actual_value.as_ref()),
    };

    direct_value
        .or_else(|| {
            // Check SubObject's uniqueIDRef
            model_sub_obj
                .unique_id_ref
                .as_ref()
                .and_then(|id_ref| param_map.get(id_ref))
                .and_then(|param| resolve_value_from_param(param, mode, template_map))
        })
        .or_else(|| {
            // If SubIndex is 0, check parent Object's uniqueIDRef.
            // This often handles the case where a Parameter definition (e.g. an Array)
            // has a default value which implies the count (SubIndex 0).
            if sub_index == 0 {
                parent_unique_id_ref
                    .and_then(|id_ref| param_map.get(id_ref))
                    .and_then(|param| resolve_value_from_param(param, mode, template_map))
            } else {
                None
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model;
    use crate::model::app_layers::{Object, SubObject};
    use crate::model::app_process::{
        AllowedValues as ModelAllowedValues, Range as ModelRange, Value as ModelValue,
    };
    use crate::types;
    use alloc::collections::BTreeMap;
    use alloc::string::ToString;
    use alloc::vec;

    // --- MOCK DATA FACTORIES ---

    fn create_test_param(
        id: &'static str,
        default_val: Option<&'static str>,
        actual_val: Option<&'static str>,
        template_ref: Option<&'static str>,
        access: Option<model::app_process::ParameterAccess>,
        support: Option<model::app_process::ParameterSupport>,
        persistent: bool,
        allowed_values: Option<ModelAllowedValues>,
    ) -> (String, model::app_process::Parameter) {
        (
            id.to_string(),
            model::app_process::Parameter {
                unique_id: id.to_string(),
                default_value: default_val.map(|v| ModelValue {
                    value: v.to_string(),
                    ..Default::default()
                }),
                actual_value: actual_val.map(|v| ModelValue {
                    value: v.to_string(),
                    ..Default::default()
                }),
                template_id_ref: template_ref.map(|s| s.to_string()),
                access,
                support,
                persistent,
                allowed_values,
                ..Default::default()
            },
        )
    }

    fn create_test_template(
        id: &'static str,
        val: &'static str,
    ) -> (String, model::app_process::Value) {
        (
            id.to_string(),
            ModelValue {
                value: val.to_string(),
                ..Default::default()
            },
        )
    }

    fn create_test_type_map() -> BTreeMap<String, DataTypeName> {
        let mut map = BTreeMap::new();
        map.insert("0005".to_string(), DataTypeName::Unsigned8);
        map.insert("0006".to_string(), DataTypeName::Unsigned16);
        map.insert("0007".to_string(), DataTypeName::Unsigned32);
        map.insert("001B".to_string(), DataTypeName::Unsigned64);
        map
    }

    // ... (Tests for resolve_allowed_values and get_value_str_for_object remain the same) ...
    #[test]
    fn test_get_value_str_for_object() {
        // Setup maps
        let (p1_id, p1) = create_test_param(
            "p1",
            Some("p1_default"),
            Some("p1_actual"),
            None,
            None,
            None,
            false,
            None,
        );
        let (p2_id, p2) = create_test_param(
            "p2",
            Some("p2_default"),
            None,
            None,
            None,
            None,
            false,
            None,
        );
        let (p3_id, p3) = create_test_param("p3", None, None, Some("t1"), None, None, false, None);

        let mut param_map = BTreeMap::new();
        param_map.insert(&p1_id, &p1);
        param_map.insert(&p2_id, &p2);
        param_map.insert(&p3_id, &p3);

        let (t1_id, t1) = create_test_template("t1", "t1_val");
        let mut template_map = BTreeMap::new();
        template_map.insert(&t1_id, &t1);

        // 1. Prioritize direct actualValue (XDC mode)
        let obj1 = Object {
            actual_value: Some("obj_actual".to_string()),
            ..Default::default()
        };
        assert_eq!(
            get_value_str_for_object(&obj1, ValueMode::Actual, &param_map, &template_map),
            Some(&"obj_actual".to_string())
        );

        // 2. Fallback to direct defaultValue (XDC mode)
        let obj2 = Object {
            default_value: Some("obj_default".to_string()),
            ..Default::default()
        };
        assert_eq!(
            get_value_str_for_object(&obj2, ValueMode::Actual, &param_map, &template_map),
            Some(&"obj_default".to_string())
        );

        // 3. Prioritize direct defaultValue (XDD mode)
        let obj3 = Object {
            actual_value: Some("obj_actual".to_string()),
            default_value: Some("obj_default".to_string()),
            ..Default::default()
        };
        assert_eq!(
            get_value_str_for_object(&obj3, ValueMode::Default, &param_map, &template_map),
            Some(&"obj_default".to_string())
        );

        // 4. Resolve from uniqueIDRef (XDC mode, param has actual)
        let obj4 = Object {
            unique_id_ref: Some("p1".to_string()),
            ..Default::default()
        };
        assert_eq!(
            get_value_str_for_object(&obj4, ValueMode::Actual, &param_map, &template_map),
            Some(&"p1_actual".to_string())
        );

        // 5. Resolve from uniqueIDRef (XDD mode, param has default)
        let obj5 = Object {
            unique_id_ref: Some("p2".to_string()),
            ..Default::default()
        };
        assert_eq!(
            get_value_str_for_object(&obj5, ValueMode::Default, &param_map, &template_map),
            Some(&"p2_default".to_string())
        );

        // 6. Resolve from uniqueIDRef via template (XDD mode)
        let obj6 = Object {
            unique_id_ref: Some("p3".to_string()),
            ..Default::default()
        };
        assert_eq!(
            get_value_str_for_object(&obj6, ValueMode::Default, &param_map, &template_map),
            Some(&"t1_val".to_string())
        );

        // 7. No value found
        let obj7 = Object::default();
        assert_eq!(
            get_value_str_for_object(&obj7, ValueMode::Default, &param_map, &template_map),
            None
        );
    }

    // --- INTEGRATION TEST for resolve_object_dictionary ---

    #[test]
    fn test_resolve_object_dictionary_full() {
        // --- Setup Mocks ---
        let type_map = create_test_type_map();

        // Setup template map
        let (t1_id, t1) = create_test_template("t1_range", "100"); // For param p_range
        let mut template_map = BTreeMap::new();
        template_map.insert(&t1_id, &t1);

        // Setup param map
        let allowed_vals = ModelAllowedValues {
            range: vec![ModelRange {
                min_value: ModelValue {
                    value: "10".to_string(),
                    ..Default::default()
                },
                max_value: ModelValue {
                    value: "50".to_string(),
                    ..Default::default()
                },
                ..Default::default()
            }],
            ..Default::default()
        };
        let (p_var_id, p_var) = create_test_param(
            "p_var",
            Some("0xAA"),
            None,
            None,
            Some(model::app_process::ParameterAccess::ReadWrite),
            Some(model::app_process::ParameterSupport::Mandatory),
            true,
            None,
        );
        let (p_sub_id, p_sub) = create_test_param(
            "p_sub",
            Some("0xBB"),
            None,
            None,
            Some(model::app_process::ParameterAccess::Read),
            Some(model::app_process::ParameterSupport::Optional),
            false,
            Some(allowed_vals),
        );
        let (p_range_id, p_range) = create_test_param(
            "p_range",
            None,
            None,
            Some("t1_range"),
            None,
            None,
            false,
            None,
        );

        let mut param_map = BTreeMap::new();
        param_map.insert(&p_var_id, &p_var);
        param_map.insert(&p_sub_id, &p_sub);
        param_map.insert(&p_range_id, &p_range);

        // Setup model ApplicationLayers
        let app_layers = model::app_layers::ApplicationLayers {
            object_list: model::app_layers::ObjectList {
                object: vec![
                    // 1. VAR with direct value (U32)
                    Object {
                        index: "1000".to_string(),
                        name: "DeviceType".to_string(),
                        object_type: "7".to_string(),
                        data_type: Some("0007".to_string()), // U32 (4 bytes)
                        default_value: Some("0x1234".to_string()), // 2-byte hex string
                        ..Default::default()
                    },
                    // 2. VAR resolving from uniqueIDRef
                    Object {
                        index: "2000".to_string(),
                        name: "ParamVar".to_string(),
                        object_type: "7".to_string(),
                        data_type: Some("0005".to_string()), // U8
                        unique_id_ref: Some("p_var".to_string()),
                        ..Default::default()
                    },
                    // 3. RECORD resolving sub-objects from uniqueIDRef
                    Object {
                        index: "2100".to_string(),
                        name: "ParamRecord".to_string(),
                        object_type: "9".to_string(),
                        sub_object: vec![
                            // Sub-obj 0: direct value (decimal)
                            SubObject {
                                sub_index: "00".to_string(),
                                name: "Count".to_string(),
                                object_type: "7".to_string(),
                                data_type: Some("0005".to_string()),
                                default_value: Some("2".to_string()), // "2"
                                ..Default::default()
                            },
                            // Sub-obj 1: param ref (hex)
                            SubObject {
                                sub_index: "01".to_string(),
                                name: "ParamSub".to_string(),
                                object_type: "7".to_string(),
                                data_type: Some("0005".to_string()),
                                unique_id_ref: Some("p_sub".to_string()),
                                ..Default::default()
                            },
                            // Sub-obj 2: template ref (decimal)
                            SubObject {
                                sub_index: "02".to_string(),
                                name: "TemplateSub".to_string(),
                                object_type: "7".to_string(),
                                data_type: Some("0005".to_string()), // U8
                                unique_id_ref: Some("p_range".to_string()),
                                ..Default::default()
                            },
                        ],
                        ..Default::default()
                    },
                    // 4. Object with bad data type for validation
                    Object {
                        index: "6000".to_string(),
                        name: "BadVar".to_string(),
                        object_type: "7".to_string(),
                        data_type: Some("0007".to_string()), // U32 (4 bytes)
                        // FIX: This value is now accepted as a valid string,
                        // validation is deferred to converter.rs
                        default_value: Some("0x0102030405".to_string()), // 5 bytes
                        ..Default::default()
                    },
                ],
            },
            ..Default::default()
        };

        // --- Run Resolver ---
        // We run in XDD mode (ValueMode::Default)
        let od = resolve_object_dictionary(
            &app_layers,
            &param_map,
            &template_map,
            &type_map,
            ValueMode::Default,
        )
        .expect("Resolver should pass now without type validation");

        // 1. Check Obj 0x1000 (Direct value)
        let obj_1000 = od.objects.iter().find(|o| o.index == 0x1000).unwrap();
        assert_eq!(obj_1000.name, "DeviceType");
        assert_eq!(obj_1000.data.as_deref(), Some("0x1234"));
        assert_eq!(obj_1000.access_type, None); // No param ref
        assert_eq!(obj_1000.support, None);
        assert_eq!(obj_1000.persistent, false);

        // 2. Check Obj 0x2000 (Param ref value and attributes)
        let obj_2000 = od.objects.iter().find(|o| o.index == 0x2000).unwrap();
        assert_eq!(obj_2000.name, "ParamVar");
        assert_eq!(obj_2000.data.as_deref(), Some("0xAA")); // from p_var default "0xAA"
        assert_eq!(
            obj_2000.access_type,
            Some(types::ParameterAccess::ReadWrite)
        );
        assert_eq!(obj_2000.support, Some(types::ParameterSupport::Mandatory));
        assert_eq!(obj_2000.persistent, true);

        // 3. Check Obj 0x2100 (Record)
        let obj_2100 = od.objects.iter().find(|o| o.index == 0x2100).unwrap();
        assert_eq!(obj_2100.name, "ParamRecord");
        assert_eq!(obj_2100.data, None); // Data is on sub-objects
        assert_eq!(obj_2100.sub_objects.len(), 3);

        // Sub-obj 0 (Direct value)
        let sub_0 = &obj_2100.sub_objects[0];
        assert_eq!(sub_0.sub_index, 0);
        assert_eq!(sub_0.data.as_deref(), Some("2")); // "2" (decimal)
        assert_eq!(sub_0.access_type, None); // No param ref

        // Sub-obj 1 (Param ref value and attributes)
        let sub_1 = &obj_2100.sub_objects[1];
        assert_eq!(sub_1.sub_index, 1);
        assert_eq!(sub_1.data.as_deref(), Some("0xBB")); // from p_sub default "0xBB"
        assert_eq!(sub_1.access_type, Some(types::ParameterAccess::ReadOnly));
        assert_eq!(sub_1.support, Some(types::ParameterSupport::Optional));
        assert_eq!(sub_1.persistent, false);
        // Check allowedValues resolution
        assert!(sub_1.allowed_values.is_some());
        let av = sub_1.allowed_values.as_ref().unwrap();
        assert_eq!(av.ranges.len(), 1);
        assert_eq!(av.ranges[0].min_value, "10");
        assert_eq!(av.ranges[0].max_value, "50");

        // Sub-obj 2 (Template ref value)
        let sub_2 = &obj_2100.sub_objects[2];
        assert_eq!(sub_2.sub_index, 2);
        assert_eq!(sub_2.data.as_deref(), Some("100")); // "100" (decimal)
        assert_eq!(sub_2.access_type, None); // Param p_range had no access type

        // 4. Check Obj 0x6000 (BadVar) - now accepted as string
        let obj_6000 = od.objects.iter().find(|o| o.index == 0x6000).unwrap();
        assert_eq!(obj_6000.name, "BadVar");
        assert_eq!(obj_6000.data.as_deref(), Some("0x0102030405"));
    }
}
