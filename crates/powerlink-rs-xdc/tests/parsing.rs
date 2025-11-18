// crates/powerlink-rs-xdc/tests/parsing.rs

use powerlink_rs_xdc::{
    ParameterAccess, load_xdc_from_str, load_xdd_defaults_from_str, save_xdc_to_string,
};
use std::fs;
use std::path::PathBuf;

/// Helper function to load a test file from the `tests/data/` directory.
fn load_test_file(name: &str) -> String {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests");
    path.push("data");
    path.push(name);

    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read test file {:?}: {}", path, e))
}

/// This test validates that the resolver correctly loads data from the
/// <ApplicationProcess> block and applies it to the <ObjectList>
/// by resolving the `uniqueIDRef`.
#[test]
fn test_resolve_extended_app_process() {
    let xml_content = load_test_file("MyDevice_extended.xdd");
    let xdc_file = load_xdd_defaults_from_str(&xml_content).expect("Failed to parse extended XDD");

    // 1. Find the Object Dictionary
    let od = &xdc_file.object_dictionary;

    // 2. Find Object 0x2100, which references "ID_Parameter1"
    let obj_2100 = od
        .objects
        .iter()
        .find(|o| o.index == 0x2100)
        .expect("Failed to find object 0x2100");

    // 3. Assert that its attributes were resolved from "ID_Parameter1"
    assert_eq!(obj_2100.name, "ExampleSimpleParameter_U8");
    assert_eq!(
        obj_2100.access_type,
        Some(ParameterAccess::ReadWrite),
        "Access type was not resolved from <parameter>"
    );
    // Assert that the `defaultValue` ("15") was correctly parsed and resolved
    // from the <parameter> (as USINT, "0005").
    // UPDATED: We now expect the string "15" directly from the XML.
    assert_eq!(
        obj_2100.data.as_deref(),
        Some("15"),
        "defaultValue was not resolved from <parameter>"
    );

    // 4. Find Object 0x2101, which references "ID_Parameter2" (a struct)
    let obj_2101 = od
        .objects
        .iter()
        .find(|o| o.index == 0x2101)
        .expect("Failed to find object 0x2101");

    // Assert its attributes were resolved
    assert_eq!(obj_2101.name, "ExampleStructure_DOM");
    assert_eq!(
        obj_2101.access_type,
        Some(ParameterAccess::ReadWrite),
        "Access type was not resolved from <parameter>"
    );
    // The parameter itself has no value, so the data should be None
    assert_eq!(obj_2101.data, None);
}

/// This test validates the full "round-trip" capability.
/// 1. Load XDD (from `defaultValue`)
/// 2. Save to string (as XDC, serializing data as `actualValue`)
/// 3. Load the new string (from `actualValue`)
/// 4. Assert the two resulting structs are identical.
#[test]
fn test_round_trip_static_xdd() {
    // 1. Load the original XDD, parsing `defaultValue`
    let xdd_content = load_test_file("MyDevice_static.xdd");
    let file1 =
        load_xdd_defaults_from_str(&xdd_content).expect("Failed to parse original static XDD");

    // 2. Save it back to a new XDC-style string
    // This converts `file1.data` (from `defaultValue`) into `actualValue` attributes.
    let xdc_string_new = save_xdc_to_string(&file1).expect("Failed to serialize XDC to string");

    // 3. Load the *new* string, parsing `actualValue`
    let file2 = load_xdc_from_str(&xdc_string_new).expect("Failed to parse newly serialized XDC");

    // 4. Compare
    // This now compares the *entire* XdcFile struct, including DeviceFunction,
    // NetworkManagement, etc., validating the builder and resolver logic for all fields.
    assert_eq!(file1, file2, "XdcFile structs mismatch after round-trip");
}

/// This test simply ensures the minimal "dynamic" XDD parses correctly.
#[test]
fn test_load_dynamic_xdd() {
    let xml_content = load_test_file("MyDevice.xdd");
    let result = load_xdd_defaults_from_str(&xml_content);
    assert!(
        result.is_ok(),
        "Failed to parse dynamic XDD: {:?}",
        result.err()
    );

    let xdc_file = result.unwrap();
    // Find a known object and check its value
    let flags_obj = xdc_file
        .object_dictionary
        .objects
        .iter()
        .find(|o| o.index == 0x1F82)
        .expect("Object 0x1F82 not found");

    assert_eq!(flags_obj.name, "NMT_FeatureFlags_U32");
    // `defaultValue="0x00000045"`
    // UPDATED: We now expect the string "0x00000045" directly from the XML.
    assert_eq!(flags_obj.data.as_deref(), Some("0x00000045"));
}
