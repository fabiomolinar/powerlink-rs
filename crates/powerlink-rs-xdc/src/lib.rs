// src/lib.rs

#![no_std]
#![doc = "Parses and generates POWERLINK XDC (XML Device Configuration) files."]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::fmt;
// Add imports for macros and Write trait
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::fmt::Write;
// Need this for `from_str_radix`
use core::num::ParseIntError;

// Import our internal XML model definitions
mod model;
use model::{Iso15745ProfileContainer, SubObject};

// --- Public Data Structures ---

/// Represents the clean, binary-ready configuration data extracted from an XDC.
///
/// This struct holds the data for the key Configuration Manager (CFM) objects
/// (e.g., 0x1F22, 0x1F26, 0x1F27) required by the Managing Node.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CfmData {
    /// A list of all parsed CFM-related objects.
    pub objects: Vec<CfmObject>,
}

/// A single, binary-ready Object Dictionary entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CfmObject {
    /// The Object Dictionary index (e.g., 0x1F22).
    pub index: u16,
    /// The Object Dictionary sub-index (e.g., 0x01).
    pub sub_index: u8,
    /// The raw binary data from the `actualValue` attribute.
    pub data: Vec<u8>,
}

// --- Public API Functions ---

/// Parses an XML string slice and extracts CFM object data.
///
/// This function deserializes the XDC XML, navigates to the ObjectList,
/// finds relevant CFM objects, and parses their `actualValue`
/// hex strings into binary.
///
/// # Arguments
/// * `xml_content` - A string slice containing the full XDC XML file.
///
/// # Errors
/// Returns an `XdcError` if parsing fails, hex conversion fails, or
/// critical objects are missing.
pub fn load_xdc_from_str(xml_content: &str) -> Result<CfmData, XdcError> {
    // 1. Deserialize the raw XML string into our internal model.
    let container: Iso15745ProfileContainer = quick_xml::de::from_str(xml_content)?;

    // 2. Find the Communication Network Profile body.
    let comm_profile_body = container
        .profile
        .iter()
        .find_map(|p| {
            if p.profile_body.xsi_type.as_deref()
                == Some("ProfileBody_CommunicationNetwork_Powerlink")
            {
                Some(&p.profile_body)
            } else {
                None
            }
        })
        .ok_or(XdcError::MissingElement {
            element: "ProfileBody_CommunicationNetwork_Powerlink",
        })?;

    // 3. Get the ApplicationLayers, which contains the ObjectList.
    let app_layers =
        comm_profile_body
            .application_layers
            .as_ref()
            .ok_or(XdcError::MissingElement {
                element: "ApplicationLayers",
            })?;

    // 4. Iterate all objects and sub-objects, parsing the ones we need.
    let mut objects = Vec::new();

    for object in &app_layers.object_list.object {
        let index = parse_hex_u16(&object.index).map_err(|_| {
            XdcError::InvalidAttributeFormat {
                attribute: "index",
            }
        })?;

        // Filter for the CFM objects we care about:
        // 0x1F22: CFM_ConciseDcfList_ADOM
        // 0x1F26: CFM_ExpConfDateList_AU32
        // 0x1F27: CFM_ExpConfTimeList_AU32
        if !(index == 0x1F22 || index == 0x1F26 || index == 0x1F27) {
            continue;
        }

        for sub_object in &object.sub_object {
            let sub_index = parse_hex_u8(&sub_object.sub_index).map_err(|_| {
                XdcError::InvalidAttributeFormat {
                    attribute: "subIndex",
                }
            })?;

            // Sub-index 0 is "NumberOfEntries" and not data.
            // We must skip it to avoid trying to hex-decode its decimal value.
            if sub_index == 0 {
                continue;
            }

            // We only care about sub-objects that have an `actualValue`.
            if let Some(actual_value) = &sub_object.actual_value {
                let data = parse_hex_string(actual_value)?;

                objects.push(CfmObject {
                    index,
                    sub_index,
                    data,
                });
            }
        }
    }

    Ok(CfmData { objects })
}

/// Serializes `CfmData` into a minimal, compliant XDC XML `String`.
///
/// This function constructs a minimal boilerplate XDC structure in memory,
/// populates the `ObjectList` from the `CfmData`, formats the binary data
/// back into hex strings, and serializes it to an XML string.
///
/// # Arguments
/// * `data` - The binary-ready `CfmData` to serialize.
///
/// # Errors
/// Returns an `XdcError` if serialization fails.
pub fn save_xdc_to_string(data: &CfmData) -> Result<String, XdcError> {
    // 1. Group CfmObjects by their index to build model::Object structs.
    // We use a BTreeMap to keep the objects sorted by index in the final XML.
    let mut object_map: BTreeMap<u16, Vec<model::SubObject>> = BTreeMap::new();

    for cfm_obj in &data.objects {
        // Create the model::SubObject from the CfmObject.
        let sub_object = model::SubObject {
            sub_index: format_hex_u8(cfm_obj.sub_index), // e.g., "01"
            actual_value: Some(format_hex_string(&cfm_obj.data)?), // e.g., "0x01020304"
        };

        // Get or create the parent Vec<SubObject>
        object_map.entry(cfm_obj.index).or_default().push(sub_object);
    }

    // --- Manual XML Creation using String formatting ---
    // This is the correct no_std + alloc approach as String implements core::fmt::Write.

    let mut buf = String::new();

    // Write XML declaration
    writeln!(
        &mut buf,
        r#"<?xml version="1.0" encoding="UTF-8"?>"#
    )?;

    // <ISO15745ProfileContainer ...>
    writeln!(&mut buf, r#"<ISO15745ProfileContainer xmlns="http://www.ethernet-powerlink.org" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:schemaLocation="http://www.ethernet-powerlink.org Powerlink_Main.xsd">"#)?;

    // <ISO15745Profile>
    writeln!(&mut buf, "  <ISO15745Profile>")?;

    // <ProfileHeader />
    writeln!(&mut buf, "    <ProfileHeader />")?;

    // <ProfileBody ...>
    writeln!(
        &mut buf,
        r#"    <ProfileBody xsi:type="ProfileBody_CommunicationNetwork_Powerlink">"#
    )?;

    // <ApplicationLayers>
    writeln!(&mut buf, "      <ApplicationLayers>")?;
    // <ObjectList>
    writeln!(&mut buf, "        <ObjectList>")?;

    // Iterate our grouped objects
    for (index, mut sub_objects) in object_map {
        // Correctly pass u16, not &u16
        writeln!(
            &mut buf,
            r#"          <Object index="{:04X}" objectType="8">"#,
            index
        )?;

        // Sort sub-objects by sub-index
        sub_objects.sort_by_key(|so| {
            // We can unwrap here because we created these strings.
            parse_hex_u8(&so.sub_index).unwrap_or(0)
        });

        // Write NumberOfEntries
        writeln!(
            &mut buf,
            r#"            <SubObject subIndex="00" actualValue="{}" />"#,
            sub_objects.len()
        )?;

        // Write all data SubObjects
        for so in sub_objects {
            // We can unwrap, this `actual_value` was just created
            writeln!(
                &mut buf,
                r#"            <SubObject subIndex="{}" actualValue="{}" />"#,
                so.sub_index,
                so.actual_value.as_ref().unwrap()
            )?;
        }
        writeln!(&mut buf, "          </Object>")?;
    }

    // </ObjectList>
    writeln!(&mut buf, "        </ObjectList>")?;
    // </ApplicationLayers>
    writeln!(&mut buf, "      </ApplicationLayers>")?;
    // </ProfileBody>
    writeln!(&mut buf, "    </ProfileBody>")?;
    // </ISO15745Profile>
    writeln!(&mut buf, "  </ISO15745Profile>")?;
    // </ISO15745ProfileContainer>
    writeln!(&mut buf, "</ISO15745ProfileContainer>")?;

    Ok(buf)
}

// --- Helper Functions ---

/// Parses a "0x..." or "..." hex string into a u16.
fn parse_hex_u16(s: &str) -> Result<u16, ParseIntError> {
    let trimmed = s.strip_prefix("0x").unwrap_or(s);
    u16::from_str_radix(trimmed, 16)
}

/// Parses a "0x..." or "..." hex string into a u8.
fn parse_hex_u8(s: &str) -> Result<u8, ParseIntError> {
    let trimmed = s.strip_prefix("0x").unwrap_or(s);
    u8::from_str_radix(trimmed, 16)
}

/// Parses a "0x..." or "..." hex string into a Vec<u8>.
fn parse_hex_string(s: &str) -> Result<Vec<u8>, XdcError> {
    let trimmed = s.strip_prefix("0x").unwrap_or(s);
    hex::decode(trimmed).map_err(XdcError::HexParsing)
}

/// Formats a u16 into an uppercase hex string (e.g., 0x1F22 -> "1F22").
fn format_hex_u16(val: u16) -> String {
    format!("{:04X}", val)
}

/// Formats a u8 into an uppercase hex string (e.g., 1 -> "01").
fn format_hex_u8(val: u8) -> String {
    format!("{:02X}", val)
}

/// Formats a byte slice into an "0x..." hex string.
fn format_hex_string(data: &[u8]) -> Result<String, XdcError> {
    let mut s = String::with_capacity(2 + data.len() * 2);
    s.push_str("0x");
    for &byte in data {
        // This write! is infallible for String.
        write!(&mut s, "{:02X}", byte)?;
    }
    Ok(s)
}

// --- Public Error Type ---

/// Errors that can occur during XDC parsing or serialization.
#[derive(Debug)]
pub enum XdcError {
    /// An error from the underlying `quick-xml` deserializer.
    XmlParsing(quick_xml::DeError),

    /// An error from the underlying `quick-xml` serializer.
    XmlWriting(quick_xml::Error),

    /// The `actualValue` attribute contained invalid hex.
    HexParsing(hex::FromHexError),

    /// An error occurred during string formatting.
    FmtError(fmt::Error),

    /// A required XML element was missing (e.g., ProfileBody).
    MissingElement { element: &'static str },

    /// A required attribute was missing (e.g., @index).
    MissingAttribute { attribute: &'static str },

    /// An attribute (e.g., @index) had an invalid format.
    InvalidAttributeFormat { attribute: &'static str },

    /// Functionality is not yet implemented.
    NotImplemented,
}

impl From<quick_xml::DeError> for XdcError {
    fn from(e: quick_xml::DeError) -> Self {
        XdcError::XmlParsing(e)
    }
}

impl From<quick_xml::Error> for XdcError {
    fn from(e: quick_xml::Error) -> Self {
        XdcError::XmlWriting(e)
    }
}

impl From<hex::FromHexError> for XdcError {
    fn from(e: hex::FromHexError) -> Self {
        XdcError::HexParsing(e)
    }
}

impl From<fmt::Error> for XdcError {
    fn from(e: fmt::Error) -> Self {
        XdcError::FmtError(e)
    }
}