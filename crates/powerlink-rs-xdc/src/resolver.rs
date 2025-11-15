// crates/powerlink-rs-xdc/src/resolver.rs

//! Handles the business logic of resolving values from a deserialized XDC/XDD model.
//!
//! This includes:
//! 1. Resolving ProfileHeader.
//! 2. Resolving DeviceIdentity.
//! 3. Resolving NetworkManagement.
//! 4. Building template and parameter maps from ApplicationProcess (Pass 1 & 2).
//! 5. Building the DataType map from ApplicationLayers (Pass 2.5).
//! 6. Resolving the rich ObjectDictionary (Pass 3).
//! 7. Validating data types and lengths.

use crate::error::XdcError;
use crate::model;
// Use full paths to the new sub-modules
use crate::model::app_layers::DataTypeName;
use crate::model::common::{AttributedGlabels, Glabels, LabelChoice};
use crate::model::Iso15745ProfileContainer;
use crate::parser::{parse_hex_u16, parse_hex_u32, parse_hex_u8, parse_hex_string};
use crate::types; // Import the new public types
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Defines which value to prioritize when resolving the Object Dictionary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ValueMode {
    /// Prioritize `actualValue` (for XDC files).
    Actual,
    /// Prioritize `defaultValue` (for XDD files).
    Default,
}

/// Resolves the final `XdcFile` data from the raw deserialized container.
/// This function contains all the logic for mapping the internal `model`
/// to the public, ergonomic `types`.
pub(crate) fn resolve_data(
    container: Iso15745ProfileContainer,
    mode: ValueMode,
) -> Result<types::XdcFile, XdcError> {
    // Find the distinct profile bodies.
    
    // The Device Profile contains Identity and ApplicationProcess.
    let device_profile = container
        .profile
        .iter()
        .find(|p| p.profile_body.device_identity.is_some());

    // The Comm Profile contains ApplicationLayers and NetworkManagement.
    let comm_profile = container
        .profile
        .iter()
        .find(|p| p.profile_body.application_layers.is_some())
        .ok_or(XdcError::MissingElement {
            element: "Profile containing ApplicationLayers",
        })?;
    
    // Get the bodies from the profiles
    let device_profile_body = device_profile.map(|p| &p.profile_body);
    let comm_profile_body = &comm_profile.profile_body;

    // --- Pass 1: Build Template Map ---
    // Stores a map of template uniqueID -> &Value element
    let mut template_map: BTreeMap<&String, &model::app_process::Value> = BTreeMap::new();

    // ApplicationProcess is in the Device Profile
    let app_process = device_profile_body.and_then(|b| b.application_process.as_ref());

    if let Some(app_process) = app_process {
        if let Some(template_list) = &app_process.template_list {
            for template in &template_list.parameter_template {
                // The value is chosen based on the parser's mode
                let value = match mode {
                    ValueMode::Actual => template.actual_value.as_ref().or(template.default_value.as_ref()),
                    ValueMode::Default => template.default_value.as_ref().or(template.actual_value.as_ref()),
                };

                if let Some(val) = value {
                    template_map.insert(&template.unique_id, val);
                }
            }
        }
    }
    // --- End of Pass 1 ---

    // --- Pass 2: Build Parameter Map (with template resolution) ---
    // Stores a map of parameter uniqueID -> &Parameter element
    let mut param_map: BTreeMap<&String, &model::app_process::Parameter> = BTreeMap::new();

    if let Some(app_process) = app_process {
        if let Some(param_list) = &app_process.parameter_list {
            for param in &param_list.parameter {
                // Just store a reference to the parameter itself.
                // Value resolution will happen in Pass 3.
                param_map.insert(&param.unique_id, param);
            }
        }
    }
    // --- End of Pass 2 ---

    // --- Pass 2.5: Build Data Type Map ---
    let app_layers = comm_profile_body
        .application_layers
        .as_ref()
        .ok_or(XdcError::MissingElement {
            element: "ApplicationLayers",
        })?;

    let mut type_map: BTreeMap<String, DataTypeName> = BTreeMap::new();
    if let Some(data_type_list) = &app_layers.data_type_list {
        for def_type in &data_type_list.def_type {
            type_map.insert(def_type.data_type.clone(), def_type.type_name);
        }
    }
    // --- End of Pass 2.5 ---

    // 4. --- Pass 3: Resolve all public-facing types ---

    // The header is mandatory in both profiles; prefer the device one.
    let header_model = device_profile
        .map(|p| &p.profile_header)
        .unwrap_or(&comm_profile.profile_header);
    
    let header = resolve_header(header_model)?;
    
    let identity = device_profile_body
        .and_then(|b| b.device_identity.as_ref())
        .map(resolve_identity)
        .transpose()?
        .unwrap_or_default();

    let network_management = comm_profile_body
        .network_management
        .as_ref()
        .map(resolve_network_management)
        .transpose()?;

    let object_dictionary =
        resolve_object_dictionary(app_layers, &param_map, &template_map, &type_map, mode)?;
    
    // 5. --- Assemble final XdcFile ---
    Ok(types::XdcFile {
        header,
        identity,
        network_management,
        object_dictionary,
    })
}

/// Parses a `model::ProfileHeader` into a `types::ProfileHeader`.
fn resolve_header(model: &model::header::ProfileHeader) -> Result<types::ProfileHeader, XdcError> {
    Ok(types::ProfileHeader {
        identification: model.profile_identification.clone(),
        revision: model.profile_revision.clone(),
        name: model.profile_name.clone(),
        source: model.profile_source.clone(),
        date: model.profile_date.clone(),
    })
}

/// Helper to extract the first available `<label>` value from a `g_labels` group.
fn extract_label_from_glabels(labels: &Glabels) -> Option<String> {
    labels.items.iter().find_map(|item| {
        if let LabelChoice::Label(label) = item {
            Some(label.value.clone())
        } else {
            None
        }
    })
}

/// Helper to extract the first available `<label>` value from an `AttributedGlabels` struct.
fn extract_label_from_attributed_glabels(attributed_labels: &AttributedGlabels) -> Option<String> {
    extract_label_from_glabels(&attributed_labels.labels)
}

/// Parses a `model::DeviceIdentity` into a clean `types::Identity`.
/// (Updated for Task 4)
fn resolve_identity(model: &model::identity::DeviceIdentity) -> Result<types::Identity, XdcError> {
    let vendor_id = model
        .vendor_id
        .as_ref()
        .map(|v| parse_hex_u32(&v.value))
        .transpose()?
        .unwrap_or(0);

    // Try hex first, fall back to decimal if parsing fails (productID is often decimal)
    let product_id = model
        .product_id
        .as_ref()
        .map(|p| {
            parse_hex_u32(&p.value)
                .or_else(|_| p.value.parse::<u32>().map_err(|_| XdcError::InvalidAttributeFormat { attribute: "productID" } ))
                .ok()
        })
        .flatten()
        .unwrap_or(0);

    let versions = model
        .version
        .iter()
        .map(|v| types::Version {
            version_type: v.version_type.clone(),
            value: v.value.clone(),
        })
        .collect();
        
    let order_number = model
        .order_number
        .iter()
        .map(|on| on.value.clone())
        .collect();

    Ok(types::Identity {
        vendor_id,
        product_id,
        vendor_name: model.vendor_name.value.clone(),
        product_name: model.product_name.value.clone(),
        versions,
        
        // --- New fields ---
        vendor_text: model.vendor_text.as_ref().and_then(extract_label_from_attributed_glabels),
        device_family: model.device_family.as_ref().and_then(extract_label_from_attributed_glabels),
        product_family: model.product_family.as_ref().map(|pf| pf.value.clone()),
        product_text: model.product_text.as_ref().and_then(extract_label_from_attributed_glabels),
        order_number,
        build_date: model.build_date.clone(),
        specification_revision: model.specification_revision.as_ref().map(|sr| sr.value.clone()),
        instance_name: model.instance_name.as_ref().map(|i| i.value.clone()),
    })
}

/// Parses a `model::NetworkManagement` into a `types::NetworkManagement`.
fn resolve_network_management(
    model: &model::net_mgmt::NetworkManagement,
) -> Result<types::NetworkManagement, XdcError> {
    let general_features = types::GeneralFeatures {
        dll_feature_mn: model.general_features.dll_feature_mn,
        nmt_boot_time_not_active: model.general_features.nmt_boot_time_not_active.clone(),
    };

    let mn_features = model.mn_features.as_ref().map(|mn| types::MnFeatures {
        nmt_mn_max_cyc_in_sync: mn.nmt_mn_max_cyc_in_sync.clone(),
        nmt_mn_pres_max: mn.nmt_mn_pres_max.clone(),
    });

    let cn_features = model.cn_features.as_ref().map(|cn| types::CnFeatures {
        nmt_cn_pre_op2_to_ready2_op: cn.nmt_cn_pre_op2_to_ready2_op.clone(),
        nmt_cn_dna: cn.nmt_cn_dna.map(|dna| dna == model::net_mgmt::CnFeaturesNmtCnDna::ClearOnPreOp1ToPreOp2),
    });

    let diagnostic = model.diagnostic.as_ref().map(resolve_diagnostic).transpose()?;

    Ok(types::NetworkManagement {
        general_features,
        mn_features,
        cn_features,
        diagnostic,
    })
}

/// Parses a `model::Diagnostic` into a `types::Diagnostic`.
fn resolve_diagnostic(model: &model::net_mgmt::Diagnostic) -> Result<types::Diagnostic, XdcError> {
    let errors = model
        .error_list
        .as_ref()
        .map_or(Vec::new(), |list| {
            list.error
                .iter()
                .map(|e| types::ErrorDefinition {
                    name: e.name.clone(),
                    label: e.label.clone(),
                    description: e.description.clone(),
                    error_type: e.error_type.clone(),
                    value: e.value.clone(),
                })
                .collect()
        });

    Ok(types::Diagnostic { errors })
}

/// Iterates the `model::ObjectList` and resolves it into a rich, public `types::ObjectDictionary`.
/// (Updated for Task 8 & 9)
fn resolve_object_dictionary<'a>(
    app_layers: &'a model::app_layers::ApplicationLayers,
    param_map: &'a BTreeMap<&'a String, &'a model::app_process::Parameter>,
    template_map: &'a BTreeMap<&'a String, &'a model::app_process::Value>,
    type_map: &BTreeMap<String, DataTypeName>, // Fix: Add type_map as argument
    mode: ValueMode,
) -> Result<types::ObjectDictionary, XdcError> {
    let mut od_objects = Vec::new();

    for model_obj in &app_layers.object_list.object {
        let index = parse_hex_u16(&model_obj.index)?;

        // --- Start: Resolve Object Attributes (Task 9) ---
        // Set defaults from the <Object> tag itself
        let mut resolved_access = model_obj.access_type.map(map_access_type);
        let mut resolved_support = None;
        let mut resolved_persistent = false;
        let mut object_data: Option<Vec<u8>> = None;
        let mut od_sub_objects: Vec<types::SubObject> = Vec::new();

        // Check if a parameter reference overrides these attributes
        if let Some(id_ref) = model_obj.unique_id_ref.as_ref() {
            if let Some(param) = param_map.get(id_ref) {
                resolved_access = param.access.map(map_param_access);
                resolved_support = param.support.map(map_param_support);
                resolved_persistent = param.persistent;
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
                validate_type(index, 0, data, data_type_id, type_map)?;
            }
        } else {
            // This is a RECORD or ARRAY. Process its <SubObject> children.
            for model_sub_obj in &model_obj.sub_object {
                let sub_index = parse_hex_u8(&model_sub_obj.sub_index)?;

                // --- Start: Resolve SubObject Attributes (Task 9) ---
                let mut sub_resolved_access = model_sub_obj.access_type.map(map_access_type);
                let mut sub_resolved_support = None;
                let mut sub_resolved_persistent = false;

                // Check if a parameter reference overrides these attributes
                if let Some(id_ref) = model_sub_obj.unique_id_ref.as_ref() {
                     if let Some(param) = param_map.get(id_ref) {
                        sub_resolved_access = param.access.map(map_param_access);
                        sub_resolved_support = param.support.map(map_param_support);
                        sub_resolved_persistent = param.persistent;
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
                    validate_type(index, sub_index, data, data_type_id, type_map)?;
                }
                
                let pdo_mapping = model_sub_obj.pdo_mapping.map(map_pdo_mapping);

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
                    support: sub_resolved_support, // Use resolved value
                    persistent: sub_resolved_persistent, // Use resolved value
                    data,
                });
            }
        }
        
        let pdo_mapping = model_obj.pdo_mapping.map(map_pdo_mapping);

        od_objects.push(types::Object {
            index,
            name: model_obj.name.clone(),
            object_type: model_obj.object_type.clone(),
            data_type: model_obj.data_type.clone(),
            low_limit: model_obj.low_limit.clone(),
            high_limit: model_obj.high_limit.clone(),
            access_type: resolved_access, // Use resolved value
            pdo_mapping,
            obj_flags: model_obj.obj_flags.clone(),
            support: resolved_support, // Use resolved value
            persistent: resolved_persistent, // Use resolved value
            data: object_data,
            sub_objects: od_sub_objects,
        });
    }

    Ok(types::ObjectDictionary {
        objects: od_objects,
    })
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
/// (Updated for Task 8 & 9)
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
/// (Updated for Task 8 & 9)
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

/// Validates that the length of the parsed data matches the expected
/// size of the given `dataType` ID.
fn validate_type(
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
fn get_data_type_size(
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
fn map_access_type(model: model::app_layers::ObjectAccessType) -> types::ParameterAccess {
    match model {
        model::app_layers::ObjectAccessType::ReadOnly => types::ParameterAccess::ReadOnly,
        model::app_layers::ObjectAccessType::WriteOnly => types::ParameterAccess::WriteOnly,
        model::app_layers::ObjectAccessType::ReadWrite => types::ParameterAccess::ReadWrite,
        model::app_layers::ObjectAccessType::Constant => types::ParameterAccess::Constant,
    }
}

/// Maps the internal model enum (`ObjectPdoMapping`) to the public types enum.
fn map_pdo_mapping(model: model::app_layers::ObjectPdoMapping) -> types::ObjectPdoMapping {
    match model {
        model::app_layers::ObjectPdoMapping::No => types::ObjectPdoMapping::No,
        model::app_layers::ObjectPdoMapping::Default => types::ObjectPdoMapping::Default,
        model::app_layers::ObjectPdoMapping::Optional => types::ObjectPdoMapping::Optional,
        model::app_layers::ObjectPdoMapping::Tpdo => types::ObjectPdoMapping::Tpdo,
        model::app_layers::ObjectPdoMapping::Rpdo => types::ObjectPdoMapping::Rpdo,
    }
}

/// Maps the `ApplicationProcess` `ParameterAccess` enum to the public `types` enum.
fn map_param_access(model: model::app_process::ParameterAccess) -> types::ParameterAccess {
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
fn map_param_support(model: model::app_process::ParameterSupport) -> types::ParameterSupport {
    match model {
        model::app_process::ParameterSupport::Mandatory => types::ParameterSupport::Mandatory,
        model::app_process::ParameterSupport::Optional => types::ParameterSupport::Optional,
        model::app_process::ParameterSupport::Conditional => types::ParameterSupport::Conditional,
    }
}