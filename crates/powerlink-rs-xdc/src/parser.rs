// crates/powerlink-rs-xdc/src/parser.rs

//! The internal XML parser and helper functions for parsing hex strings.

use crate::error::XdcError;
use crate::model;
use crate::resolver; // This module's functions are now called by `load_...`
use crate::resolver::ValueMode; // Import the new ValueMode enum
use crate::types::XdcFile;
use alloc::string::String;
use alloc::vec::Vec;
use core::num::ParseIntError;
use hex::FromHexError;
use quick_xml::de::from_str;

// --- Public API Functions ---

/// Loads XDC data (using `actualValue`) from an XML string.
///
/// This function parses the XML and resolves the data model by prioritizing
/// the `actualValue` attributes, which is standard for XDC (Configuration) files.
pub fn load_xdc_from_str(s: &str) -> Result<XdcFile, XdcError> {
    let container = parse_xml_str(s)?;
    // Call the resolver with ValueMode::Actual
    resolver::resolve_data(container, ValueMode::Actual)
}

/// Loads XDD default data (using `defaultValue`) from an XML string.
///
/// This function parses the XML and resolves the data model by prioritizing
/// the `defaultValue` attributes, which is standard for XDD (Device Description) files.
pub fn load_xdd_defaults_from_str(s: &str) -> Result<XdcFile, XdcError> {
    let container = parse_xml_str(s)?;
    // Call the resolver with ValueMode::Default
    resolver::resolve_data(container, ValueMode::Default)
}

// --- Internal XML Deserialization ---

/// The core internal function that uses `quick-xml` to deserialize the string
/// into the raw `model` structs.
pub(crate) fn parse_xml_str(s: &str) -> Result<model::Iso15745ProfileContainer, XdcError> {
    // quick-xml's deserializer is very efficient.
    // It maps the XML structure directly to our `model` structs.
    from_str(s).map_err(XdcError::from)
}

// --- Hex String Parsing Helpers ---
// These are used by the resolver.

/// Parses a "0x..." or "..." hex string into a `u32`.
pub(crate) fn parse_hex_u32(s: &str) -> Result<u32, ParseIntError> {
    let s_no_prefix = s.strip_prefix("0x").unwrap_or(s);
    u32::from_str_radix(s_no_prefix, 16)
}

/// Parses a "0x..." or "..." hex string into a `u16`.
pub(crate) fn parse_hex_u16(s: &str) -> Result<u16, ParseIntError> {
    let s_no_prefix = s.strip_prefix("0x").unwrap_or(s);
    u16::from_str_radix(s_no_prefix, 16)
}

/// Parses a "0x..." or "..." hex string into a `u8`.
pub(crate) fn parse_hex_u8(s: &str) -> Result<u8, ParseIntError> {
    let s_no_prefix = s.strip_prefix("0x").unwrap_or(s);
    u8::from_str_radix(s_no_prefix, 16)
}

/// Parses a "0x..." or "..." hex string into a byte vector.
pub(crate) fn parse_hex_string(s: &str) -> Result<Vec<u8>, FromHexError> {
    let s_no_prefix = s.strip_prefix("0x").unwrap_or(s);
    
    // Handle odd-length strings by padding with a leading zero
    if s_no_prefix.len() % 2 != 0 {
        let mut padded_s = String::with_capacity(s_no_prefix.len() + 1);
        padded_s.push('0');
        padded_s.push_str(s_no_prefix);
        hex::decode(padded_s)
    } else {
        hex::decode(s_no_prefix)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::XdcError;

    // A minimal but complete XDC structure for testing.
    const MINIMAL_GOOD_XDC: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ISO15745ProfileContainer xmlns="http://www.ethernet-powerlink.org" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:schemaLocation="http://www.ethernet-powerlink.org Powerlink_Main.xsd">
  <ISO15745Profile>
    <ProfileHeader>
      <ProfileIdentification>Test</ProfileIdentification>
      <ProfileRevision>1.0</ProfileRevision>
      <ProfileName>Test Profile</ProfileName>
      <ProfileSource>Test</ProfileSource>
      <ProfileClassID>Device</ProfileClassID>
      <ISO15745Reference>
        <ISO15745Part>4</ISO15745Part>
        <ISO15745Edition>1</ISO15745Edition>
        <ProfileTechnology>Powerlink</ProfileTechnology>
      </ISO15745Reference>
    </ProfileHeader>
    <ProfileBody xsi:type="ProfileBody_Device_Powerlink" fileName="test.xdd" fileCreator="Test" fileCreationDate="2024-01-01" fileVersion="1">
      <DeviceIdentity>
        <vendorName>TestVendor</vendorName>
        <productName>TestProduct</productName>
      </DeviceIdentity>
    </ProfileBody>
  </ISO15745Profile>
  <ISO15745Profile>
    <ProfileHeader>
      <ProfileIdentification>Test</ProfileIdentification>
      <ProfileRevision>1.0</ProfileRevision>
      <ProfileName>Test Profile</ProfileName>
      <ProfileSource>Test</ProfileSource>
      <ProfileClassID>CommunicationNetwork</ProfileClassID>
      <ISO15745Reference>
        <ISO15745Part>4</ISO15745Part>
        <ISO15745Edition>1</ISO15745Edition>
        <ProfileTechnology>Powerlink</ProfileTechnology>
      </ISO15745Reference>
    </ProfileHeader>
    <ProfileBody xsi:type="ProfileBody_CommunicationNetwork_Powerlink" fileName="test.xdd" fileCreator="Test" fileCreationDate="2024-01-01" fileVersion="1">
      <ApplicationLayers>
        <ObjectList>
          <Object index="1000" name="Device Type" objectType="7" dataType="0006" actualValue="0x1234" />
        </ObjectList>
      </ApplicationLayers>
      <NetworkManagement>
        <GeneralFeatures DLLFeatureMN="false" NMTBootTimeNotActive="0" NMTCycleTimeMax="0" NMTCycleTimeMin="0" NMTErrorEntries="0" />
      </NetworkManagement>
    </ProfileBody>
  </ISO15745Profile>
</ISO15745ProfileContainer>"#;

    #[test]
    fn test_load_xdc_from_str_happy_path() {
        let result = load_xdc_from_str(MINIMAL_GOOD_XDC);
        assert!(result.is_ok());
        let xdc_file = result.unwrap();
        // Fix: Use `name` field, not `profile_name`
        assert_eq!(xdc_file.header.name, "Test Profile");
        assert_eq!(xdc_file.identity.vendor_name, "TestVendor");
        assert_eq!(xdc_file.object_dictionary.objects.len(), 1);
        assert_eq!(xdc_file.object_dictionary.objects[0].index, 0x1000);
        // Fix: Explicitly cast the array reference to a slice `&[u8]`
        assert_eq!(xdc_file.object_dictionary.objects[0].data.as_deref(), Some(&[0x34u8, 0x12u8] as &[u8]));
    }

    #[test]
    fn test_load_xdd_defaults_from_str_happy_path() {
        let xdd_xml = MINIMAL_GOOD_XDC.replace("actualValue", "defaultValue");
        let result = load_xdd_defaults_from_str(&xdd_xml);
        assert!(result.is_ok());
        let xdd_file = result.unwrap();
        assert_eq!(xdd_file.identity.vendor_name, "TestVendor");
        // Fix: Explicitly cast the array reference to a slice `&[u8]`
        assert_eq!(xdd_file.object_dictionary.objects[0].data.as_deref(), Some(&[0x34u8, 0x12u8] as &[u8]));
    }

    #[test]
    fn test_load_xdc_malformed_xml() {
        let malformed_xml = "<ISO15745ProfileContainer><ProfileHeader>"; // Missing closing tags
        let result = load_xdc_from_str(malformed_xml);
        assert!(matches!(result, Err(XdcError::XmlParsing(_))));
    }

    #[test]
    fn test_load_xdc_invalid_attribute() {
        // Contains `index="1FGG"`, which is not valid hex.
        let invalid_attr_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ISO15745ProfileContainer xmlns="http://www.ethernet-powerlink.org" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:schemaLocation="http://www.ethernet-powerlink.org Powerlink_Main.xsd">
  <ISO15745Profile>
    <ProfileHeader>
      <ProfileIdentification>Test</ProfileIdentification>
      <ProfileRevision>1.0</ProfileRevision>
      <ProfileName>Test Profile</ProfileName>
      <ProfileSource>Test</ProfileSource>
      <ProfileClassID>Device</ProfileClassID>
    </ProfileHeader>
    <ProfileBody xsi:type="ProfileBody_Device_Powerlink" fileName="test.xdd" fileCreator="Test" fileCreationDate="2024-01-01" fileVersion="1">
      <DeviceIdentity><vendorName>Test</vendorName><productName>Test</productName></DeviceIdentity>
    </ProfileBody>
  </ISO15745Profile>
  <ISO15745Profile>
    <ProfileHeader>
      <ProfileIdentification>Test</ProfileIdentification>
      <ProfileRevision>1.0</ProfileRevision>
      <ProfileName>Test Profile</ProfileName>
      <ProfileSource>Test</ProfileSource>
      <ProfileClassID>CommunicationNetwork</ProfileClassID>
    </ProfileHeader>
    <ProfileBody xsi:type="ProfileBody_CommunicationNetwork_Powerlink" fileName="test.xdd" fileCreator="Test" fileCreationDate="2024-01-01" fileVersion="1">
      <ApplicationLayers>
        <ObjectList>
          <Object index="1FGG" name="Device Type" objectType="7" dataType="0007" actualValue="0x1234" />
        </ObjectList>
      </ApplicationLayers>
      <NetworkManagement>
        <GeneralFeatures DLLFeatureMN="false" NMTBootTimeNotActive="0" NMTCycleTimeMax="0" NMTCycleTimeMin="0" NMTErrorEntries="0" />
      </NetworkManagement>
    </ProfileBody>
  </ISO15745Profile>
</ISO15745ProfileContainer>"#;

        let result = load_xdc_from_str(invalid_attr_xml);
        assert!(matches!(result, Err(XdcError::InvalidAttributeFormat { attribute: "index or subIndex" })));
    }
}