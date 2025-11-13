// src/parser.rs

use crate::error::XdcError;
use crate::model::{self, SubObject}; 
use crate::types::{CfmData, CfmObject, Identity, Version, XdcFile};
use alloc::string::String;
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
    load_from_str_internal(xml_content, |so| so.actual_value.as_ref())
}

/// Parses an XDD (XML Device Description) string slice and extracts CFM object data
/// from `defaultValue` attributes.
///
/// This function is used to load the default factory configuration from
/// a device description file.
///
/// # Errors
/// Returns an `XdcError` if parsing fails, hex conversion fails, or
/// critical elements are missing.
pub fn load_xdd_defaults_from_str(xml_content: &str) -> Result<XdcFile, XdcError> {
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
    let device_profile_body =
        container
            .profile
            .iter()
            .find_map(|p| {
                let pt = p.profile_body.xsi_type.as_deref();
                if pt == Some("ProfileBody_Device_Powerlink")
                    || pt == Some("ProfileBody_Device_Powerlink_Modular_Head")
                    || pt == Some("ProfileBody_Device_Powerlink_Modular_Child")
                {
                    Some(&p.profile_body)
                } else {
                    None
                }
            })
            .ok_or(XdcError::MissingElement {
                element: "ProfileBody_Device_Powerlink",
            })?;

    let identity = device_profile_body
        .device_identity
        .as_ref()
        .map(parse_identity)
        .transpose()?
        .unwrap_or_default();

    // 3. Find and parse the ObjectList from the Communication Profile
    let comm_profile_body = container
        .profile
        .iter()
        .find_map(|p| {
            let pt = p.profile_body.xsi_type.as_deref();
            if pt == Some("ProfileBody_CommunicationNetwork_Powerlink")
                || pt == Some("ProfileBody_CommunicationNetwork_Powerlink_Modular_Head")
                || pt == Some("ProfileBody_CommunicationNetwork_Powerlink_Modular_Child")
            {
                Some(&p.profile_body)
            } else {
                None
            }
        })
        .ok_or(XdcError::MissingElement {
            element: "ProfileBody_CommunicationNetwork_Powerlink",
        })?;

    let app_layers =
        comm_profile_body
            .application_layers
            .as_ref()
            .ok_or(XdcError::MissingElement {
                element: "ApplicationLayers",
            })?;

    // 4. Iterate all objects and sub-objects, parsing the ones we need.
    // NOTE: This parser is now fully generalized, reading all objects.
    let mut objects = Vec::new();

    for object in &app_layers.object_list.object {
        // ParseIntError now converted via From impl in error.rs
        let index = parse_hex_u16(&object.index)?;

        for sub_object in &object.sub_object {
            // ParseIntError now converted via From impl in error.rs
            let sub_index = parse_hex_u8(&sub_object.sub_index)?;

            // Sub-index 0 is "NumberOfEntries" and not data.
            if sub_index == 0 {
                continue;
            }

            // We only care about sub-objects that have a value from the selector.
            if let Some(value_str) = value_selector(sub_object) {
                let data = parse_hex_string(value_str)?;

                objects.push(CfmObject {
                    index,
                    sub_index,
                    data,
                });
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