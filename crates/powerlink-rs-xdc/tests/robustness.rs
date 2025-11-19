//! Integration tests focused on error handling and edge cases.
//!
//! These tests ensure the parser correctly identifies and reports errors for
//! malformed XML, invalid attributes, missing mandatory elements, and data type
//! mismatches, without panicking.

use powerlink_rs_xdc::{XdcError, load_xdc_from_str, to_core_od};

/// A minimal valid XDC template used as a base for creating corrupted test cases.
const MINIMAL_VALID_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
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
      <DeviceFunction>
        <capabilities/>
      </DeviceFunction>
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
        <DataTypeList>
           <defType dataType="0006"><Unsigned16/></defType>
        </DataTypeList>
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

/// Verifies that the parser catches malformed XML syntax (e.g., unclosed tags).
#[test]
fn test_malformed_xml_syntax() {
    let xml = r#"<ISO15745ProfileContainer><ProfileHeader> ... missing closing tags"#;
    let result = load_xdc_from_str(xml);
    assert!(
        matches!(result, Err(XdcError::XmlParsing(_))),
        "Expected XmlParsing error, got {:?}",
        result
    );
}

/// Verifies that the resolver catches invalid hex strings in the `index` attribute.
#[test]
fn test_invalid_index_hex() {
    // Inject an invalid index "ZZZZ"
    let xml = MINIMAL_VALID_XML.replace(r#"index="1000""#, r#"index="ZZZZ""#);
    let result = load_xdc_from_str(&xml);

    assert!(
        matches!(
            result,
            Err(XdcError::InvalidAttributeFormat {
                attribute: "index or subIndex"
            })
        ),
        "Expected InvalidAttributeFormat error, got {:?}",
        result
    );
}

/// Verifies that the resolver requires the `ApplicationLayers` block in the Communication Profile.
#[test]
fn test_missing_application_layers() {
    // Remove the ApplicationLayers block
    let start = MINIMAL_VALID_XML.find("<ApplicationLayers>").unwrap();
    let end =
        MINIMAL_VALID_XML.find("</ApplicationLayers>").unwrap() + "</ApplicationLayers>".len();
    let mut xml = MINIMAL_VALID_XML.to_string();
    xml.replace_range(start..end, "");

    let result = load_xdc_from_str(&xml);
    assert!(
        matches!(
            result,
            Err(XdcError::MissingElement {
                element: "Profile containing ApplicationLayers"
            })
        ),
        "Expected MissingElement error for ApplicationLayers, got {:?}",
        result
    );
}

/// Verifies that the `converter` (to core OD) handles invalid data values gracefully.
///
/// The parser loads the string as-is, but the converter must fail if the string
/// cannot be parsed into the target native type.
#[test]
fn test_data_type_conversion_failure() {
    // Inject invalid data "NOT_HEX" for U16 type
    let xml = MINIMAL_VALID_XML.replace(r#"actualValue="0x1234""#, r#"actualValue="NOT_HEX""#);

    // 1. Load should succeed (the parser stores it as a String).
    let xdc_file = load_xdc_from_str(&xml).expect("Parser should handle raw strings");

    // 2. Conversion to Core OD should FAIL.
    let core_result = to_core_od(&xdc_file);

    assert!(
        matches!(
            core_result,
            Err(XdcError::InvalidAttributeFormat {
                attribute: "defaultValue or actualValue (numeric)"
            })
        ),
        "Expected conversion error for invalid numeric string, got {:?}",
        core_result
    );
}

/// Verifies that the `converter` handles boolean parsing correctly across variants.
#[test]
fn test_boolean_conversion_variants() {
    let base_xml = MINIMAL_VALID_XML
        .replace(
            r#"<defType dataType="0006"><Unsigned16/></defType>"#,
            r#"<defType dataType="0001"><Boolean/></defType>"#,
        )
        .replace(
            r#"dataType="0006" actualValue="0x1234""#,
            r#"dataType="0001" actualValue="REPLACE_ME""#,
        );

    // Case 1: "true" -> 1
    let xml_true = base_xml.replace("REPLACE_ME", "true");
    let file_true = load_xdc_from_str(&xml_true).unwrap();
    let od_true = to_core_od(&file_true).unwrap();
    let val_true = od_true.read_object(0x1000).unwrap();

    if let powerlink_rs::od::Object::Variable(powerlink_rs::od::ObjectValue::Boolean(v)) = val_true
    {
        assert_eq!(*v, 1);
    } else {
        panic!("Expected Boolean(1)");
    }

    // Case 2: "0" -> 0
    let xml_zero = base_xml.replace("REPLACE_ME", "0");
    let file_zero = load_xdc_from_str(&xml_zero).unwrap();
    let od_zero = to_core_od(&file_zero).unwrap();
    let val_zero = od_zero.read_object(0x1000).unwrap();
    if let powerlink_rs::od::Object::Variable(powerlink_rs::od::ObjectValue::Boolean(v)) = val_zero
    {
        assert_eq!(*v, 0);
    } else {
        panic!("Expected Boolean(0)");
    }

    // Case 3: Invalid "yes" -> Error
    let xml_invalid = base_xml.replace("REPLACE_ME", "yes");
    let file_invalid = load_xdc_from_str(&xml_invalid).unwrap();
    let res_invalid = to_core_od(&file_invalid);
    assert!(matches!(
        res_invalid,
        Err(XdcError::InvalidAttributeFormat { .. })
    ));
}

/// Verifies resilience against broken `uniqueIDRef` links.
///
/// The resolver should treat broken links as "no value found" rather than panicking.
#[test]
fn test_broken_parameter_reference() {
    let xml = MINIMAL_VALID_XML.replace(
        r#"<Object index="1000" name="Device Type" objectType="7" dataType="0006" actualValue="0x1234" />"#,
        r#"<Object index="1000" name="Device Type" objectType="7" uniqueIDRef="NON_EXISTENT_ID" />"#
    );

    let result = load_xdc_from_str(&xml);
    assert!(result.is_ok(), "Parser should not panic on broken ref");

    let file = result.unwrap();
    let obj = &file.object_dictionary.objects[0];

    // Since the ref was broken and no direct value provided, data should be None
    assert_eq!(obj.data, None);
}

/// Verifies that XML entities are correctly decoded.
#[test]
fn test_xml_entity_decoding() {
    let xml = MINIMAL_VALID_XML.replace(
        r#"<vendorName>TestVendor</vendorName>"#,
        r#"<vendorName>B&amp;R Automation</vendorName>"#,
    );

    let xdc_file = load_xdc_from_str(&xml).expect("Failed to parse XML with entities");
    assert_eq!(xdc_file.identity.vendor_name, "B&R Automation");
}

/// Verifies that numeric values overflowing their target types trigger an error.
#[test]
fn test_numeric_overflow() {
    // Set type to Unsigned8 (0005) but value to 256
    let base_xml = MINIMAL_VALID_XML
        .replace(
            r#"<defType dataType="0006"><Unsigned16/></defType>"#,
            r#"<defType dataType="0005"><Unsigned8/></defType>"#,
        )
        .replace(
            r#"dataType="0006" actualValue="0x1234""#,
            r#"dataType="0005" actualValue="256""#,
        );

    let xdc_file = load_xdc_from_str(&base_xml).expect("Initial parse should succeed");

    // Conversion should fail
    let core_result = to_core_od(&xdc_file);

    assert!(
        matches!(
            core_result,
            Err(XdcError::InvalidAttributeFormat {
                attribute: "defaultValue or actualValue (numeric)"
            })
        ),
        "Expected numeric overflow/format error, got {:?}",
        core_result
    );
}

/// Verifies behavior when mandatory fields in ProfileHeader are missing.
#[test]
fn test_missing_header_fields() {
    let xml =
        MINIMAL_VALID_XML.replace(r#"<ProfileIdentification>Test</ProfileIdentification>"#, "");

    let result = load_xdc_from_str(&xml);
    // Quick-xml struct deserialization should fail because the field is not Option<>
    assert!(
        matches!(result, Err(XdcError::XmlParsing(_))),
        "Expected XmlParsing error for missing mandatory field, got {:?}",
        result
    );
}

/// Verifies behavior when an unknown data type ID is used.
#[test]
fn test_unknown_data_type() {
    // Use a made-up data type ID "FFFF"
    let xml = MINIMAL_VALID_XML.replace(r#"dataType="0006""#, r#"dataType="FFFF""#);

    let xdc_file = load_xdc_from_str(&xml).expect("Parse should succeed");

    // Converter attempts to map this, fails to find logic for "FFFF", returns error.
    let core_result = to_core_od(&xdc_file);

    assert!(
        matches!(core_result, Err(XdcError::ValidationError(_))),
        "Expected ValidationError for unknown data type, got {:?}",
        core_result
    );
}
