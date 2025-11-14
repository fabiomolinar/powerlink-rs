// crates/powerlink-rs-xdc/src/resolver.rs

//! Handles the business logic of resolving values from a deserialized XDC/XDD model.
//!
//! This includes:
//! 1. Parsing DeviceIdentity.
//! 2. Building template and parameter maps from ApplicationProcess (Pass 1 & 2).
//! 3. Building the DataType map from ApplicationLayers (Pass 2.5).
//! 4. Resolving Object/SubObject values using uniqueIDRef (Pass 3).
//! 5. Validating data types and lengths.

use crate::error::XdcError;
use crate::model::{self, DataTypeName, Iso15745ProfileContainer, SubObject};
use crate::parser::{parse_hex_u16, parse_hex_u32, parse_hex_u8, parse_hex_string};
use crate::types::{CfmData, CfmObject, Identity, Version, XdcFile};
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Resolves the final `XdcFile` data from the raw deserialized container.
/// This function contains all the logic moved from `parser.rs`.
pub(crate) fn resolve_data(
    container: Iso15745ProfileContainer,
    value_selector: impl Fn(&SubObject) -> Option<&String>,
) -> Result<XdcFile, XdcError> {
    // 1. Find and parse the Device Identity from the Device Profile
    let identity = container
        .profile
        .iter()
        .find_map(|p| p.profile_body.device_identity.as_ref())
        .map(parse_identity)
        .transpose()?
        .unwrap_or_default();

    // --- Pass 1: Build Template Map ---
    let mut template_map: BTreeMap<String, String> = BTreeMap::new();

    // Find the Device Profile body, which contains the ApplicationProcess
    let app_process = container
        .profile
        .iter()
        .find_map(|p| p.profile_body.application_process.as_ref());

    if let Some(app_process) = app_process {
        if let Some(template_list) = &app_process.template_list {
            for template in &template_list.parameter_template {
                // We only care about templates that have a uniqueID and a defaultValue
                if let Some(default_val) = &template.default_value {
                    template_map.insert(template.unique_id.clone(), default_val.value.clone());
                }
            }
        }
    }
    // --- End of Pass 1 ---

    // --- Pass 2: Build Parameter Map (with template resolution) ---
    let mut param_map: BTreeMap<String, String> = BTreeMap::new();
    
    if let Some(app_process) = app_process {
        if let Some(param_list) = &app_process.parameter_list {
            for param in &param_list.parameter {
                // A parameter's value can come from:
                // 1. Its own `defaultValue` element (highest priority).
                // 2. A `defaultValue` inherited from its `templateIDRef` (fallback).
                let value_opt = param
                    .default_value
                    .as_ref()
                    .map(|v| v.value.clone())
                    .or_else(|| {
                        // If no direct value, try resolving templateIDRef
                        param
                            .template_id_ref
                            .as_ref()
                            .and_then(|template_id| template_map.get(template_id).cloned())
                    });
                
                if let Some(value) = value_opt {
                    param_map.insert(param.unique_id.clone(), value);
                }
            }
        }
    }
    // --- End of Pass 2 ---


    // 3. Find and parse the ObjectList from the Communication Profile
    let app_layers = container
        .profile
        .iter()
        .find_map(|p| p.profile_body.application_layers.as_ref())
        .ok_or(XdcError::MissingElement {
            element: "ApplicationLayers",
        })?;

    // --- NEW: Pass 2.5 - Build Data Type Map ---
    let mut type_map: BTreeMap<String, DataTypeName> = BTreeMap::new();
    if let Some(data_type_list) = &app_layers.data_type_list {
        for def_type in &data_type_list.def_type {
            type_map.insert(def_type.data_type.clone(), def_type.type_name);
        }
    }
    // --- End of Pass 2.5 ---

    // 4. --- Pass 3: Iterate objects and resolve values ---
    let mut objects = Vec::new();

    for object in &app_layers.object_list.object {
        let index = parse_hex_u16(&object.index)?;

        for sub_object in &object.sub_object {
            let sub_index = parse_hex_u8(&sub_object.sub_index)?;

            // Logic to find the correct value string:
            // 1. Try the direct selector (e.g., `actualValue` or `defaultValue` on the SubObject)
            // 2. If that's None, try resolving the SubObject's `uniqueIDRef`
            // 3. If that's None, try resolving the parent Object's `uniqueIDRef` (applies to SubIndex 00 only per spec)
            
            let value_str_opt = value_selector(sub_object)
                .or_else(|| {
                    // Try to resolve SubObject's uniqueIDRef
                    sub_object.unique_id_ref.as_ref().and_then(|id_ref| {
                        param_map.get(id_ref).map(|val_str| val_str)
                    })
                })
                .or_else(|| {
                    // If still None, and we are sub-index 0, check the parent Object's uniqueIDRef
                    if sub_index == 0 {
                         object.unique_id_ref.as_ref().and_then(|id_ref| {
                            param_map.get(id_ref).map(|val_str| val_str)
                        })
                    } else {
                        None
                    }
                });

            // --- Type Validation Logic ---
            let effective_data_type_id = sub_object.data_type.as_deref().or(object.data_type.as_deref());

            
            // We only care about sub-objects that have a value (either direct or resolved)
            if let Some(value_str) = value_str_opt {
                // Try to parse the value as hex.
                // This will fail for "NumberOfEntries" (e.g., "2"), which is fine.
                // We only want to store binary CFM data.
                if let Ok(data) = parse_hex_string(value_str) {
                    // --- Perform validation check ---
                    if let Some(data_type_id_str) = effective_data_type_id {
                        // Pass the new type_map to the validation function
                        if let Some(expected_len) = get_data_type_size(data_type_id_str, &type_map) {
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
                    }
                    // --- End of validation check ---
                    
                     objects.push(CfmObject {
                        index,
                        sub_index,
                        data,
                    });
                }
            }
        }
    }

    Ok(XdcFile {
        identity,
        data: CfmData { objects },
    })
}

/// Parses a `model::DeviceIdentity` into a clean `types::Identity`.
/// (Moved from parser.rs)
fn parse_identity(model: &model::DeviceIdentity) -> Result<Identity, XdcError> {
    let vendor_id = model
        .vendor_id
        .as_ref()
        .map(|v| parse_hex_u32(v))
        .transpose()?
        .unwrap_or(0);

    // Try hex first, fall back to decimal if parsing fails (productID is often decimal)
    let product_id = model
        .product_id
        .as_ref()
        .map(|p| parse_hex_u32(p).or_else(|_| p.parse().map_err(|e| e).map(|x| x).map(|x: u32| x)).ok())
        .flatten()
        .unwrap_or(0);

    let versions = model
        .version
        .iter()
        .map(|v| Version {
            version_type: v.version_type.clone(),
            value: v.value.clone(),
        })
        .collect();

    Ok(Identity {
        vendor_id,
        product_id,
        vendor_name: model.vendor_name.clone(),
        product_name: model.product_name.clone(),
        versions,
    })
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
            "0013" => Some(6), // Integer48
            "0014" => Some(7), // Integer56
            "0015" => Some(8), // Integer64
            "0016" => Some(3), // Unsigned24
            "0017" => Some(5), // Unsigned40
            "0018" => Some(6), // Unsigned48
            "0019" => Some(7), // Unsigned56
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
            | "001A" // BITSTRING
            => None,
            // Unknown types:
            _ => None,
        }
    }
}