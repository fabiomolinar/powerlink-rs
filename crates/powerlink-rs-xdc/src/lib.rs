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

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    /// A minimal, valid XDC XML string for testing.
    /// Contains objects 0x1006 (to be ignored) and the CFM objects
    /// 0x1F22, 0x1F26, and 0x1F27.
    const TEST_XDC_CONTENT: &str = r#"
<?xml version="1.0" encoding="UTF-8"?>
<ISO15745ProfileContainer xmlns="http://www.ethernet-powerlink.org" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:schemaLocation="http://www.ethernet-powerlink.org Powerlink_Main.xsd">
  <ISO15745Profile>
    <ProfileHeader>
    </ProfileHeader>
    <ProfileBody xsi:type="ProfileBody_Device_Powerlink">
      <ApplicationLayers>
        <ObjectList>
          <Object index="6000" objectType="7" />
        </ObjectList>
      </ApplicationLayers>
    </ProfileBody>
  </ISO15745Profile>
  <ISO15745Profile>
    <ProfileHeader>
    </ProfileHeader>
    <ProfileBody xsi:type="ProfileBody_CommunicationNetwork_Powerlink">
      <ApplicationLayers>
        <ObjectList>
          <Object index="1006" name="NMT_CycleLen_U32" objectType="7">
            <SubObject subIndex="00" name="Cycle Time" actualValue="10000" />
          </Object>
          
          <Object index="1F22" name="CFM_ConciseDcfList_ADOM" objectType="8">
            <SubObject subIndex="00" name="NumberOfEntries" actualValue="1" />
            <SubObject subIndex="01" name="Dcf_1" actualValue="0x0102030405060708" />
          </Object>
          
          <Object index="1F26" name="CFM_ExpConfDateList_AU32" objectType="8">
            <SubObject subIndex="01" name="Node_1_Date" actualValue="0x11223344" />
            <SubObject subIndex="02" name="Node_2_Date" defaultValue="0" />
          </Object>
          
          <Object index="1F27" name="CFM_ExpConfTimeList_AU32" objectType="8">
            <SubObject subIndex="01" name="Node_1_Time" actualValue="0x55667788" />
            <SubObject subIndex="02" name="Node_2_Time" actualValue="0xAABBCCDD" />
          </Object>
        </ObjectList>
      </ApplicationLayers>
    </ProfileBody>
  </ISO15745Profile>
</ISO15745ProfileContainer>
    "#;

    #[test]
    fn test_load_xdc() {
        let cfm_data =
            load_xdc_from_str(TEST_XDC_CONTENT).expect("Failed to parse test XDC");

        // We expect 4 CfmObjects in total (1 from 1F22, 1 from 1F26, 2 from 1F27)
        assert_eq!(cfm_data.objects.len(), 4);

        // Check Object 0x1F22, SubObject 0x01
        let obj_1f22 = cfm_data
            .objects
            .iter()
            .find(|o| o.index == 0x1F22 && o.sub_index == 1)
            .expect("Did not find 0x1F22/01");
        assert_eq!(
            obj_1f22.data,
            vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08]
        );

        // Check Object 0x1F26, SubObject 0x01
        let obj_1f26 = cfm_data
            .objects
            .iter()
            .find(|o| o.index == 0x1F26 && o.sub_index == 1)
            .expect("Did not find 0x1F26/01");
        assert_eq!(obj_1f26.data, vec![0x11, 0x22, 0x33, 0x44]);

        // Check Object 0x1F27, SubObject 0x01
        let obj_1f27_1 = cfm_data
            .objects
            .iter()
            .find(|o| o.index == 0x1F27 && o.sub_index == 1)
            .expect("Did not find 0x1F27/01");
        assert_eq!(obj_1f27_1.data, vec![0x55, 0x66, 0x77, 0x88]);

        // Check Object 0x1F27, SubObject 0x02
        let obj_1f27_2 = cfm_data
            .objects
            .iter()
            .find(|o| o.index == 0x1F27 && o.sub_index == 2)
            .expect("Did not find 0x1F27/02");
        assert_eq!(obj_1f27_2.data, vec![0xAA, 0xBB, 0xCC, 0xDD]);

        // Check that ignored objects are not present
        assert!(cfm_data
            .objects
            .iter()
            .find(|o| o.index == 0x1006)
            .is_none());
        assert!(cfm_data
            .objects
            .iter()
            .find(|o| o.index == 0x6000)
            .is_none());
        // Check that sub-object without actualValue is not present
        assert!(cfm_data
            .objects
            .iter()
            .find(|o| o.index == 0x1F26 && o.sub_index == 2)
            .is_none());
    }

    #[test]
    fn test_save_xdc_to_string() {
        let cfm_data = CfmData {
            objects: vec![
                CfmObject {
                    index: 0x1F22,
                    sub_index: 1,
                    data: vec![0x01, 0x02, 0x03],
                },
                // Add an object out of order to test sorting
                CfmObject {
                    index: 0x1F26,
                    sub_index: 1,
                    data: vec![0xAA, 0xBB],
                },
                CfmObject {
                    index: 0x1F22,
                    sub_index: 2,
                    data: vec![0x04, 0x05],
                },
            ],
        };

        let xml_string =
            save_xdc_to_string(&cfm_data).expect("Failed to save XDC to string");

        // Check for XML declaration
        assert!(xml_string.starts_with(r#"<?xml version="1.0" encoding="UTF-8"?>"#));
        // Check for root element
        assert!(xml_string
            .contains(r#"<ISO15745ProfileContainer xmlns="http://www.ethernet-powerlink.org""#));
        // Check for profile body
        assert!(xml_string
            .contains(r#"<ProfileBody xsi:type="ProfileBody_CommunicationNetwork_Powerlink">"#));
        // Check for 0x1F22
        assert!(xml_string.contains(r#"<Object index="1F22" objectType="8">"#));
        assert!(
            xml_string.contains(r#"<SubObject subIndex="00" actualValue="2" />"#)
        );
        assert!(
            xml_string.contains(r#"<SubObject subIndex="01" actualValue="0x010203" />"#)
        );
        assert!(
            xml_string.contains(r#"<SubObject subIndex="02" actualValue="0x0405" />"#)
        );
        // Check for 0x1F26
        assert!(xml_string.contains(r#"<Object index="1F26" objectType="8">"#));
        assert!(
            xml_string.contains(r#"<SubObject subIndex="00" actualValue="1" />"#)
        );
        assert!(
            xml_string.contains(r#"<SubObject subIndex="01" actualValue="0xAABB" />"#)
        );
    }

    #[test]
    fn test_round_trip() {
        // 1. Load from the canonical test string
        let data_a =
            load_xdc_from_str(TEST_XDC_CONTENT).expect("Round-trip: Initial load failed");

        // 2. Save it back to a new string
        let xml_b = save_xdc_to_string(&data_a)
            .expect("Round-trip: Save failed");

        // 3. Load from the newly generated string
        let data_b =
            load_xdc_from_str(&xml_b).expect("Round-trip: Second load failed");

        // 4. The data must be identical.
        // We must sort both lists to ensure a stable comparison.
        let mut objects_a = data_a.objects;
        let mut objects_b = data_b.objects;

        objects_a.sort_by_key(|o| (o.index, o.sub_index));
        objects_b.sort_by_key(|o| (o.index, o.sub_index));
        
        assert_eq!(objects_a, objects_b, "Round-trip data does not match");
    }

    #[test]
    fn test_load_errors() {
        // Test malformed XML
        let bad_xml = "<Object";
        let result = load_xdc_from_str(bad_xml);
        assert!(matches!(result, Err(XdcError::XmlParsing(_))));

        // Test valid XML but missing the correct profile
        let missing_profile_xml = r#"
<ISO15745ProfileContainer xmlns="http://www.ethernet-powerlink.org" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <ISO15745Profile>
    <ProfileBody xsi:type="ProfileBody_Device_Powerlink">
    </ProfileBody>
  </ISO15745Profile>
</ISO15745ProfileContainer>
        "#;
        let result = load_xdc_from_str(missing_profile_xml);
        assert!(matches!(
            result,
            Err(XdcError::MissingElement {
                element: "ProfileBody_CommunicationNetwork_Powerlink"
            })
        ));

        // Test invalid hex (odd length)
        let odd_hex_xml = r#"
<ISO15745ProfileContainer xmlns="http://www.ethernet-powerlink.org" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <ISO15745Profile>
    <ProfileBody xsi:type="ProfileBody_CommunicationNetwork_Powerlink">
      <ApplicationLayers>
        <ObjectList>
          <Object index="1F22" objectType="8">
            <SubObject subIndex="01" actualValue="0x123" />
          </Object>
        </ObjectList>
      </ApplicationLayers>
    </ProfileBody>
  </ISO15745Profile>
</ISO15745ProfileContainer>
        "#;
        let result = load_xdc_from_str(odd_hex_xml);
        assert!(matches!(result, Err(XdcError::HexParsing(_))));
        if let Err(XdcError::HexParsing(e)) = result {
             assert_eq!(e, hex::FromHexError::OddLength);
        }

        // Test invalid hex (bad char)
        let bad_char_xml = r#"
<ISO15745ProfileContainer xmlns="http://www.ethernet-powerlink.org" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <ISO15745Profile>
    <ProfileBody xsi:type="ProfileBody_CommunicationNetwork_Powerlink">
      <ApplicationLayers>
        <ObjectList>
          <Object index="1F22" objectType="8">
            <SubObject subIndex="01" actualValue="0xGGHH" />
          </Object>
        </ObjectList>
      </ApplicationLayers>
    </ProfileBody>
  </ISO15745Profile>
</ISO15745ProfileContainer>
        "#;
        let result = load_xdc_from_str(bad_char_xml);
        assert!(matches!(result, Err(XdcError::HexParsing(_))));
        if let Err(XdcError::HexParsing(e)) = result {
             assert_eq!(e, hex::FromHexError::InvalidHexCharacter { c: 'G', index: 0 });
        }
    }
}