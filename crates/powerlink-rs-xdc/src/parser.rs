// crates/powerlink-rs-xdc/src/parser.rs

use crate::error::XdcError;
use crate::model::{self, SubObject}; 
use crate::types::{CfmData, CfmObject, Identity, Version, XdcFile};
use alloc::collections::BTreeMap; // <-- Import BTreeMap
use alloc::string::{String, ToString}; // <-- Import ToString
use alloc::vec::Vec;
use core::num::ParseIntError;

/// Parses an XDC (XML Device Configuration) string slice and extracts CFM object data
/// from `actualValue` attributes.
///
/// This function is used to load a device's final configuration.
///
/// # Arguments
/// * `xml_content` - A string slice containing the full XDC XML file.
///
/// # Errors
/// Returns an `XdcError` if parsing fails, hex conversion fails, or
/// critical elements are missing.
pub fn load_xdc_from_str(xml_content: &str) -> Result<XdcFile, XdcError> {
    // For XDC (configuration), we only care about `actualValue`.
    // We do not resolve `uniqueIDRef` as `actualValue` is required to be present.
    load_from_str_internal(xml_content, |so| so.actual_value.as_ref())
}

/// Parses an XDD (XML Device Description) string slice and extracts CFM object data
/// from `defaultValue` attributes.
///
/// This function is used to load the default factory configuration from
/// a device description file. It supports resolving `uniqueIDRef` attributes
/// to the `ApplicationProcess` parameter list.
///
/// # Errors
/// Returns an `XdcError` if parsing fails, hex conversion fails, or
/// critical elements are missing.
pub fn load_xdd_defaults_from_str(xml_content: &str) -> Result<XdcFile, XdcError> {
    // For XDD (description), we prioritize `defaultValue` but will
    // fall back to resolving `uniqueIDRef` if it's not present.
    load_from_str_internal(xml_content, |so| so.default_value.as_ref())
}

/// Internal parsing logic that accepts a closure to select the correct
/// attribute (`actualValue` or `defaultValue`).
fn load_from_str_internal(
    xml_content: &str,
    value_selector: impl Fn(&SubObject) -> Option<&String>,
) -> Result<XdcFile, XdcError> {
    // 1. Deserialize the raw XML string into our internal model.
    let container: model::Iso15745ProfileContainer = quick_xml::de::from_str(xml_content)?;

    // 2. Find and parse the Device Identity from the Device Profile
    let identity = container
        .profile
        .iter()
        .find_map(|p| p.profile_body.device_identity.as_ref())
        .map(parse_identity)
        .transpose()?
        .unwrap_or_default();

    // --- NEW: Pass 1 - Build Parameter Map ---
    // Create a lookup map for uniqueID -> defaultValue string
    let mut param_map: BTreeMap<String, String> = BTreeMap::new();
    
    // Find the Device Profile body, which contains the ApplicationProcess
    if let Some(device_profile_body) = container.profile.iter().find(|p| {
        p.profile_body.device_identity.is_some() || p.profile_body.application_process.is_some()
    }) {
        if let Some(app_process) = &device_profile_body.profile_body.application_process {
            if let Some(param_list) = &app_process.parameter_list {
                for param in &param_list.parameter {
                    // We only care about parameters that have a uniqueID and a defaultValue
                    if let Some(default_val) = &param.default_value {
                        param_map.insert(param.unique_id.clone(), default_val.value.clone());
                    }
                }
            }
        }
    }
    // --- End of Pass 1 ---


    // 3. Find and parse the ObjectList from the Communication Profile
    let app_layers = container
        .profile
        .iter()
        .find_map(|p| p.profile_body.application_layers.as_ref())
        .ok_or(XdcError::MissingElement {
            element: "ApplicationLayers",
        })?;

    // 4. --- NEW: Pass 2 - Iterate objects and resolve values ---
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

            // --- NEW: Type Validation Logic ---
            let effective_data_type = sub_object.data_type.as_deref().or(object.data_type.as_deref());

            
            // We only care about sub-objects that have a value (either direct or resolved)
            if let Some(value_str) = value_str_opt {
                // Try to parse the value as hex.
                // This will fail for "NumberOfEntries" (e.g., "2"), which is fine.
                // We only want to store binary CFM data.
                if let Ok(data) = parse_hex_string(value_str) {
                    // --- NEW: Perform validation check ---
                    if let Some(data_type_str) = effective_data_type {
                        if let Some(expected_len) = get_data_type_size(data_type_str) {
                            if data.len() != expected_len {
                                return Err(XdcError::TypeValidationError {
                                    index,
                                    sub_index,
                                    data_type: data_type_str.to_string(),
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
/// Returns `None` for variable-sized types (like strings) or unknown types.
fn get_data_type_size(data_type: &str) -> Option<usize> {
    match data_type {
        "0001" => Some(1), // Boolean
        "0002" => Some(1), // Integer8
        "0005" => Some(1), // Unsigned8
        "0003" => Some(2), // Integer16
        "0006" => Some(2), // Unsigned16
        "0004" => Some(4), // Integer32
        "0007" => Some(4), // Unsigned32
        "0008" => Some(4), // Real32
        "0010" => Some(3), // Integer24
        "0016" => Some(3), // Unsigned24
        "0012" => Some(5), // Integer40
        "0018" => Some(5), // Unsigned40
        "0013" => Some(6), // Integer48
        "0019" => Some(6), // Unsigned48
        "0014" => Some(7), // Integer56
        "001A" => Some(7), // Unsigned56
        "0011" => Some(8), // Real64
        "0015" => Some(8), // Integer64
        "001B" => Some(8), // Unsigned64
        "0401" => Some(6), // MAC ADDRESS
        "0402" => Some(4), // IP ADDRESS
        "0403" => Some(8), // NETTIME
        // Variable-sized types:
        "0009" | "000A" | "000B" | "000D" | "000F" => None,
        // Unknown types:
        _ => None,
    }
}

// --- Helper Functions (Public for use in builder.rs) ---

/// Parses a "0x..." or "..." hex string into a u32.
pub fn parse_hex_u32(s: &str) -> Result<u32, ParseIntError> {
    let trimmed = s.strip_prefix("0x").unwrap_or(s);
    u32::from_str_radix(trimmed, 16)
}

/// Parses a "0x..." or "..." hex string into a u16.
pub fn parse_hex_u16(s: &str) -> Result<u16, ParseIntError> {
    let trimmed = s.strip_prefix("0x").unwrap_or(s);
    u16::from_str_radix(trimmed, 16)
}

/// Parses a "0x..." or "..." hex string into a u8.
pub fn parse_hex_u8(s: &str) -> Result<u8, ParseIntError> {
    let trimmed = s.strip_prefix("0x").unwrap_or(s);
    u8::from_str_radix(trimmed, 16)
}

/// Parses a "0x..." or "..." hex string into a Vec<u8>.
pub fn parse_hex_string(s: &str) -> Result<Vec<u8>, XdcError> {
    let trimmed = s.strip_prefix("0x").unwrap_or(s);
    if trimmed.len() % 2 != 0 {
        return Err(XdcError::HexParsing(hex::FromHexError::OddLength));
    }
    hex::decode(trimmed).map_err(XdcError::HexParsing)
}