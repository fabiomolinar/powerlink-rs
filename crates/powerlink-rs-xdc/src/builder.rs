// src/builder.rs

use crate::error::XdcError;
use crate::model; 
use crate::types::XdcFile;
use alloc::collections::BTreeMap;
use alloc::fmt;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;

/// Serializes `XdcFile` data into a minimal, compliant XDC XML `String`.
///
/// This function constructs a minimal boilerplate XDC structure in memory,
/// populates the `DeviceIdentity` and `ObjectList` from the `XdcFile` struct,
/// formats binary data back into hex strings, and serializes it to an XML string.
///
/// This function only serializes to `actualValue` attributes, as is standard
/// for an XDC file.
///
/// # Arguments
/// * `file` - The `XdcFile` data to serialize.
///
/// # Errors
/// Returns an `XdcError` if serialization fails or string formatting fails.
pub fn save_xdc_to_string(file: &XdcFile) -> Result<String, XdcError> {
    // 1. Group CfmObjects by their index to build model::Object structs.
    // We use a BTreeMap to keep the objects sorted by index in the final XML.
    let mut object_map: BTreeMap<u16, Vec<model::SubObject>> = BTreeMap::new();

    for cfm_obj in &file.data.objects {
        // Create the model::SubObject from the CfmObject.
        let sub_object = model::SubObject {
            // FIX: Call local helper functions for formatting
            sub_index: format_hex_u8(cfm_obj.sub_index), 
            actual_value: Some(format_hex_string(&cfm_obj.data)?), 
            default_value: None, // We only write actualValue for XDCs
        };

        // Get or create the parent Vec<SubObject>
        object_map.entry(cfm_obj.index).or_default().push(sub_object);
    }

    // --- Manual XML Creation using String formatting ---
    
    let mut buf = String::new();

    // Write XML declaration
    writeln!(
        &mut buf,
        r#"<?xml version="1.0" encoding="UTF-8"?>"#
    )?;

    // <ISO15745ProfileContainer ...>
    writeln!(&mut buf, r#"<ISO15745ProfileContainer xmlns="http://www.ethernet-powerlink.org" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:schemaLocation="http://www.ethernet-powerlink.org Powerlink_Main.xsd">"#)?;

    // --- Device Profile ---
    writeln!(&mut buf, "  <ISO15745Profile>")?;
    writeln!(&mut buf, "    <ProfileHeader />")?;
    writeln!(
        &mut buf,
        r#"    <ProfileBody xsi:type="ProfileBody_Device_Powerlink">"#
    )?;
    writeln!(&mut buf, "      <DeviceIdentity>")?;
    if let Some(name) = &file.identity.vendor_name {
        writeln!(&mut buf, "        <vendorName>{}</vendorName>", name)?;
    }
    writeln!(
        &mut buf,
        "        <vendorID>0x{:08X}</vendorID>",
        file.identity.vendor_id
    )?;
    if let Some(name) = &file.identity.product_name {
        writeln!(&mut buf, "        <productName>{}</productName>", name)?;
    }
    writeln!(
        &mut buf,
        "        <productID>0x{:08X}</productID>",
        file.identity.product_id
    )?;
    for version in &file.identity.versions {
        writeln!(
            &mut buf,
            r#"        <version versionType="{}" value="{}" />"#,
            version.version_type, version.value
        )?;
    }
    writeln!(&mut buf, "      </DeviceIdentity>")?;
    writeln!(&mut buf, "    </ProfileBody>")?;
    writeln!(&mut buf, "  </ISO15745Profile>")?;

    // --- Communication Profile ---
    writeln!(&mut buf, "  <ISO15745Profile>")?;
    writeln!(&mut buf, "    <ProfileHeader />")?;
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
        // Correctly pass u16
        writeln!(
            &mut buf,
            r#"          <Object index="{:04X}" objectType="8">"#,
            index
        )?;

        // Sort sub-objects by sub-index
        sub_objects.sort_by_key(|so| {
            // FIX: Call the public parser function
            super::parser::parse_hex_u8(&so.sub_index).unwrap_or(0)
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