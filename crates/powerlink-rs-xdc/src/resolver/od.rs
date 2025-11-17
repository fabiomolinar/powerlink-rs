// crates/powerlink-rs-xdc/src/resolver/od.rs

use crate::error::XdcError;
use crate::model;
use crate::model::app_layers::DataTypeName;
use crate::parser::{parse_hex_u16, parse_hex_u8}; // Removed parse_hex_string
use crate::resolver::{utils, ValueMode};
use crate::types;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::num::ParseIntError; // Import for new helper
use hex::FromHexError; // Import for new helper

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
            let data_type_id = model_obj.data_type.as_deref();

            // We only store data if it's valid.
            object_data =
                value_str_opt.and_then(|s| parse_value_to_bytes(s, data_type_id, type_map).ok());

            // Perform type validation if we have data
            if let (Some(data), Some(data_type_id_str)) = (object_data.as_ref(), data_type_id) {
                utils::validate_type(index, 0, data, data_type_id_str, type_map)?;
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
                let data_type_id = model_sub_obj.data_type.as_deref();

                // We only store data if it's valid.
                let data =
                    value_str_opt.and_then(|s| parse_value_to_bytes(s, data_type_id, type_map).ok());

                // Perform type validation if we have data
                if let (Some(data), Some(data_type_id_str)) = (data.as_ref(), data_type_id) {
                    utils::validate_type(index, sub_index, data, data_type_id_str, type_map)?;
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

/// Parses a value string (e.g., "0x1234", "100") into a `Vec<u8>`
/// using little-endian byte order, based on the `dataType`.
fn parse_value_to_bytes(
    s: &str,
    data_type_id: Option<&str>,
    type_map: &BTreeMap<String, DataTypeName>,
) -> Result<Vec<u8>, XdcError> {
    let id = data_type_id.ok_or(XdcError::ValidationError(
        "Cannot parse value string to bytes without dataType",
    ))?;

    // Helper to parse a decimal string to a type and get LE bytes
    macro_rules! parse_dec_le {
        ($typ:ty) => {
            s.parse::<$typ>()
                .map(|v| v.to_le_bytes().to_vec())
                .map_err(|_| XdcError::InvalidAttributeFormat {
                    attribute: "defaultValue or actualValue (decimal)",
                })
        };
    }

    if !s.starts_with("0x") {
        // Not a hex string, treat as decimal for numeric types
        match id {
            "0001" | "0002" | "0005" => parse_dec_le!(u8), // "100" -> [0x64], "2" -> [0x02]
            "0003" | "0006" => parse_dec_le!(u16),
            "0004" | "0007" => parse_dec_le!(u32),
            "0015" | "001B" => parse_dec_le!(u64),
            "0008" => parse_dec_le!(f32),
            "0011" => parse_dec_le!(f64),
            // Handle non-numeric non-hex strings
            "0009" | "000B" => Ok(s.as_bytes().to_vec()), // VisibleString, UnicodeString
            // Per spec, Octet/Domain are hex, so they must start with 0x.
            // If they don't, it's a format error.
            "000A" | "000F" => Err(XdcError::InvalidAttributeFormat {
                attribute: "defaultValue or actualValue (non-hex OctetString/Domain)",
            }),
            _ => {
                // Fallback for types not listed.
                // This handles cases like "2" for sub-index 0 (U8).
                parse_dec_le!(u8)
            }
        }
    } else {
        // It *is* a hex string.
        // For string/domain types, parse as raw hex.
        match id {
            "0009" | "000A" | "000B" | "000F" => {
                return crate::parser::parse_hex_string(s).map_err(|e| e.into())
            }
            _ => {} // Not a string type, continue
        }

        // It's a numeric hex string. Parse as number, encode as LE.
        let s_no_prefix = s.strip_prefix("0x").unwrap_or(s);
        let size_opt = utils::get_data_type_size(id, type_map);

        // FIX: Map ParseIntError to XdcError in every arm
        match size_opt {
            Some(1) => u8::from_str_radix(s_no_prefix, 16).map(|v| v.to_le_bytes().to_vec()).map_err(|e| e.into()),
            Some(2) => u16::from_str_radix(s_no_prefix, 16).map(|v| v.to_le_bytes().to_vec()).map_err(|e| e.into()),
            Some(4) => u32::from_str_radix(s_no_prefix, 16).map(|v| v.to_le_bytes().to_vec()).map_err(|e| e.into()),
            Some(8) => u64::from_str_radix(s_no_prefix, 16).map(|v| v.to_le_bytes().to_vec()).map_err(|e| e.into()),
            // Handle non-standard sizes by parsing as a large int and slicing
            Some(3) => {
                let val = u32::from_str_radix(s_no_prefix, 16)?;
                Ok(val.to_le_bytes()[..3].to_vec())
            }
            Some(5) => {
                let val = u64::from_str_radix(s_no_prefix, 16)?;
                Ok(val.to_le_bytes()[..5].to_vec())
            }
            Some(6) => {
                let val = u64::from_str_radix(s_no_prefix, 16)?;
                Ok(val.to_le_bytes()[..6].to_vec())
            }
            Some(7) => {
                let val = u64::from_str_radix(s_no_prefix, 16)?;
                Ok(val.to_le_bytes()[..7].to_vec())
            }
            _ => Err(XdcError::ValidationError("Unknown numeric type size for hex value"))
        }
    }
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


#[cfg(test)]
mod tests {
    use super::*;
    use crate::model;
    use crate::model::app_layers::{Object, SubObject};
    use crate::model::app_process::{
        AllowedValues as ModelAllowedValues, Range as ModelRange, Value as ModelValue,
    };
    use crate::model::common::{Glabels, Label, LabelChoice};
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

    // --- UNIT TESTS for resolve_allowed_values ---

    #[test]
    fn test_resolve_allowed_values() {
        let model_av = ModelAllowedValues {
            template_id_ref: None,
            value: vec![ModelValue {
                value: "1".to_string(),
                labels: Some(Glabels {
                    items: vec![LabelChoice::Label(Label {
                        lang: "en".to_string(),
                        value: "On".to_string(),
                    })],
                }),
                ..Default::default()
            }],
            range: vec![ModelRange {
                min_value: ModelValue {
                    value: "10".to_string(),
                    ..Default::default()
                },
                max_value: ModelValue {
                    value: "100".to_string(),
                    ..Default::default()
                },
                step: Some(ModelValue {
                    value: "2".to_string(),
                    ..Default::default()
                }),
            }],
        };

        let pub_av = resolve_allowed_values(&model_av).unwrap();

        assert_eq!(pub_av.values.len(), 1);
        assert_eq!(pub_av.values[0].value, "1");
        assert_eq!(pub_av.values[0].label, Some("On".to_string()));

        assert_eq!(pub_av.ranges.len(), 1);
        assert_eq!(pub_av.ranges[0].min_value, "10");
        assert_eq!(pub_av.ranges[0].max_value, "100");
        assert_eq!(pub_av.ranges[0].step, Some("2".to_string()));
    }

    // --- UNIT TESTS for get_value_str_for_object ---

    #[test]
    fn test_get_value_str_for_object() {
        // Setup maps
        let (p1_id, p1) = create_test_param("p1", Some("p1_default"), Some("p1_actual"), None, None, None, false, None);
        let (p2_id, p2) = create_test_param("p2", Some("p2_default"), None, None, None, None, false, None);
        let (p3_id, p3) = create_test_param("p3", None, None, Some("t1"), None, None, false, None);
        
        let mut param_map = BTreeMap::new();
        param_map.insert(&p1_id, &p1);
        param_map.insert(&p2_id, &p2);
        param_map.insert(&p3_id, &p3);

        let (t1_id, t1) = create_test_template("t1", "t1_val");
        let mut template_map = BTreeMap::new();
        template_map.insert(&t1_id, &t1);

        // 1. Prioritize direct actualValue (XDC mode)
        let obj1 = Object { actual_value: Some("obj_actual".to_string()), ..Default::default() };
        assert_eq!(get_value_str_for_object(&obj1, ValueMode::Actual, &param_map, &template_map), Some(&"obj_actual".to_string()));

        // 2. Fallback to direct defaultValue (XDC mode)
        let obj2 = Object { default_value: Some("obj_default".to_string()), ..Default::default() };
        assert_eq!(get_value_str_for_object(&obj2, ValueMode::Actual, &param_map, &template_map), Some(&"obj_default".to_string()));

        // 3. Prioritize direct defaultValue (XDD mode)
        let obj3 = Object { actual_value: Some("obj_actual".to_string()), default_value: Some("obj_default".to_string()), ..Default::default() };
        assert_eq!(get_value_str_for_object(&obj3, ValueMode::Default, &param_map, &template_map), Some(&"obj_default".to_string()));
        
        // 4. Resolve from uniqueIDRef (XDC mode, param has actual)
        let obj4 = Object { unique_id_ref: Some("p1".to_string()), ..Default::default() };
        assert_eq!(get_value_str_for_object(&obj4, ValueMode::Actual, &param_map, &template_map), Some(&"p1_actual".to_string()));
        
        // 5. Resolve from uniqueIDRef (XDD mode, param has default)
        let obj5 = Object { unique_id_ref: Some("p2".to_string()), ..Default::default() };
        assert_eq!(get_value_str_for_object(&obj5, ValueMode::Default, &param_map, &template_map), Some(&"p2_default".to_string()));

        // 6. Resolve from uniqueIDRef via template (XDD mode)
        let obj6 = Object { unique_id_ref: Some("p3".to_string()), ..Default::default() };
        assert_eq!(get_value_str_for_object(&obj6, ValueMode::Default, &param_map, &template_map), Some(&"t1_val".to_string()));

        // 7. No value found
        let obj7 = Object::default();
        assert_eq!(get_value_str_for_object(&obj7, ValueMode::Default, &param_map, &template_map), None);
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
                min_value: ModelValue { value: "10".to_string(), ..Default::default() },
                max_value: ModelValue { value: "50".to_string(), ..Default::default() },
                ..Default::default()
            }],
            ..Default::default()
        };
        let (p_var_id, p_var) = create_test_param("p_var", Some("0xAA"), None, None, Some(model::app_process::ParameterAccess::ReadWrite), Some(model::app_process::ParameterSupport::Mandatory), true, None);
        let (p_sub_id, p_sub) = create_test_param("p_sub", Some("0xBB"), None, None, Some(model::app_process::ParameterAccess::Read), Some(model::app_process::ParameterSupport::Optional), false, Some(allowed_vals));
        let (p_range_id, p_range) = create_test_param("p_range", None, None, Some("t1_range"), None, None, false, None);

        let mut param_map = BTreeMap::new();
        param_map.insert(&p_var_id, &p_var);
        param_map.insert(&p_sub_id, &p_sub);
        param_map.insert(&p_range_id, &p_range);

        // Setup model ApplicationLayers
        let app_layers = model::app_layers::ApplicationLayers {
            object_list: model::app_layers::ObjectList {
                object: vec![
                    // 1. VAR with direct value (U32) - THIS WILL FAIL VALIDATION
                    Object {
                        index: "1000".to_string(),
                        name: "DeviceType".to_string(),
                        object_type: "7".to_string(),
                        data_type: Some("0007".to_string()), // U32
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
                            }
                        ],
                        ..Default::default()
                    },
                    // 4. Object with bad data type for validation (5 bytes for U32)
                    Object {
                        index: "6000".to_string(),
                        name: "BadVar".to_string(),
                        object_type: "7".to_string(),
                        data_type: Some("0007".to_string()), // U32
                        default_value: Some("0x0102030405".to_string()), // 5 bytes, but type is 4
                        ..Default::default()
                    }
                ],
            },
            ..Default::default()
        };

        // --- Run Resolver ---
        // We run in XDD mode (ValueMode::Default)
        let od_result = resolve_object_dictionary(&app_layers, &param_map, &template_map, &type_map, ValueMode::Default);

        // --- Assertions ---
        // The first object (0x1000) should cause a validation error now
        assert!(od_result.is_err(), "Expected validation error due to bad data type: {:?}", od_result);
        assert!(matches!(od_result.err().unwrap(), XdcError::TypeValidationError {
            index: 0x1000,
            sub_index: 0,
            expected_bytes: 4, // U32
            actual_bytes: 2, // "0x1234"
            ..
        }));
        
        // --- Rerun with a corrected object list ---
        let mut app_layers_good = app_layers;
        // Fix object 0x1000
        app_layers_good.object_list.object[0].default_value = Some("0x12345678".to_string());
        // Fix object 0x6000 (the one that was *supposed* to fail)
        // This is the new failing object
        app_layers_good.object_list.object[3].default_value = Some("0x0102030405".to_string());
        
        // Now, 0x1000 is valid (4 bytes), but 0x6000 is invalid (5 bytes)
        let od_result_2 = resolve_object_dictionary(&app_layers_good, &param_map, &template_map, &type_map, ValueMode::Default);
        assert!(od_result_2.is_err(), "Expected validation error for 0x6000: {:?}", od_result_2);
        assert!(matches!(od_result_2.err().unwrap(), XdcError::TypeValidationError {
            index: 0x6000,
            sub_index: 0,
            expected_bytes: 4, // U32
            actual_bytes: 5, // "0x0102030405"
            ..
        }));

        // --- Rerun with all valid objects ---
        app_layers_good.object_list.object.pop(); // Remove object 0x6000
        let od = resolve_object_dictionary(&app_layers_good, &param_map, &template_map, &type_map, ValueMode::Default).unwrap();


        // 1. Check Obj 0x1000 (Direct value)
        let obj_1000 = od.objects.iter().find(|o| o.index == 0x1000).unwrap();
        assert_eq!(obj_1000.name, "DeviceType");
        assert_eq!(obj_1000.data.as_deref(), Some(&[0x12u8, 0x34, 0x56, 0x78] as &[u8])); // raw hex decode
        assert_eq!(obj_1000.access_type, None); // No param ref
        assert_eq!(obj_1000.support, None);
        assert_eq!(obj_1000.persistent, false);

        // 2. Check Obj 0x2000 (Param ref value and attributes)
        let obj_2000 = od.objects.iter().find(|o| o.index == 0x2000).unwrap();
        assert_eq!(obj_2000.name, "ParamVar");
        assert_eq!(obj_2000.data.as_deref(), Some(&[0xAAu8] as &[u8])); // from p_var default "0xAA"
        assert_eq!(obj_2000.access_type, Some(types::ParameterAccess::ReadWrite));
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
        assert_eq!(sub_0.data.as_deref(), Some(&[0x02u8] as &[u8])); // "2" (decimal)
        assert_eq!(sub_0.access_type, None); // No param ref
        
        // Sub-obj 1 (Param ref value and attributes)
        let sub_1 = &obj_2100.sub_objects[1];
        assert_eq!(sub_1.sub_index, 1);
        assert_eq!(sub_1.data.as_deref(), Some(&[0xBBu8] as &[u8])); // from p_sub default "0xBB"
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
        assert_eq!(sub_2.data.as_deref(), Some(&[100u8] as &[u8])); // "100" (decimal)
        assert_eq!(sub_2.access_type, None); // Param p_range had no access type
    }
}