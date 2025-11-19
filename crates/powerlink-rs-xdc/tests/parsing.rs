//! Integration tests for full XDC/XDD file parsing and round-trip serialization.
//!
//! These tests validate that real-world (or mock real-world) XML files are
//! parsed correctly into the public `XdcFile` structure and can be serialized
//! back to XML without data loss.

use powerlink_rs_xdc::{
    load_xdc_from_str, load_xdd_defaults_from_str, save_xdc_to_string, ParameterAccess,
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

/// Validates that the resolver correctly loads data from the `<ApplicationProcess>`
/// block and applies it to the `<ObjectList>` via `uniqueIDRef`.
#[test]
fn test_resolve_extended_app_process() {
    let xml_content = load_test_file("MyDevice_extended.xdd");
    // Load as XDD (use defaults)
    let xdc_file = load_xdd_defaults_from_str(&xml_content).expect("Failed to parse extended XDD");

    let od = &xdc_file.object_dictionary;

    // 1. Find Object 0x2100, which references "ID_Parameter1"
    let obj_2100 = od
        .objects
        .iter()
        .find(|o| o.index == 0x2100)
        .expect("Failed to find object 0x2100");

    // 2. Assert that its attributes were resolved from "ID_Parameter1"
    assert_eq!(obj_2100.name, "ExampleSimpleParameter_U8");
    assert_eq!(
        obj_2100.access_type,
        Some(ParameterAccess::ReadWrite),
        "Access type was not resolved from <parameter>"
    );
    // Assert that the `defaultValue` ("15") was correctly parsed from the <parameter>.
    assert_eq!(
        obj_2100.data.as_deref(),
        Some("15"),
        "defaultValue was not resolved from <parameter>"
    );

    // 3. Find Object 0x2101, which references "ID_Parameter2" (a struct)
    let obj_2101 = od
        .objects
        .iter()
        .find(|o| o.index == 0x2101)
        .expect("Failed to find object 0x2101");

    assert_eq!(obj_2101.name, "ExampleStructure_DOM");
    assert_eq!(
        obj_2101.access_type,
        Some(ParameterAccess::ReadWrite),
        "Access type was not resolved from <parameter>"
    );
    // The parameter itself has no value, so data should be None
    assert_eq!(obj_2101.data, None);
}

/// Validates the full "round-trip" capability.
///
/// 1. Load an XDD file (parsing `defaultValue`).
/// 2. Serialize it back to a string (generating valid XDC XML).
/// 3. Load that new string back in.
/// 4. Assert the internal structures match.
#[test]
fn test_round_trip_static_xdd() {
    // 1. Load original XDD
    let xdd_content = load_test_file("MyDevice_static.xdd");
    let file1 =
        load_xdd_defaults_from_str(&xdd_content).expect("Failed to parse original static XDD");

    // 2. Serialize to string
    let xdc_string_new = save_xdc_to_string(&file1).expect("Failed to serialize XDC to string");

    // 3. Load the new string
    // Note: `save_xdc_to_string` generally writes values to `actualValue` fields
    // if loaded from XDC, or preserves source structure.
    // For this test, we use `load_xdc_from_str` to read the result.
    let file2 = load_xdc_from_str(&xdc_string_new).expect("Failed to parse newly serialized XDC");

    // 4. Detailed Comparisons
    assert_eq!(file1.header, file2.header, "Header mismatch");
    assert_eq!(file1.identity, file2.identity, "Identity mismatch");

    // Check NetworkManagement
    if let (Some(nm1), Some(nm2)) = (&file1.network_management, &file2.network_management) {
        assert_eq!(
            nm1.general_features, nm2.general_features,
            "GeneralFeatures mismatch"
        );
        assert_eq!(nm1.mn_features, nm2.mn_features, "MnFeatures mismatch");
        assert_eq!(nm1.cn_features, nm2.cn_features, "CnFeatures mismatch");
        assert_eq!(nm1.diagnostic, nm2.diagnostic, "Diagnostic mismatch");
    } else {
        assert_eq!(
            file1.network_management.is_some(),
            file2.network_management.is_some(),
            "NetworkManagement presence mismatch"
        );
    }

    assert_eq!(
        file1.device_function, file2.device_function,
        "DeviceFunction mismatch"
    );
    assert_eq!(
        file1.device_manager, file2.device_manager,
        "DeviceManager mismatch"
    );
    assert_eq!(
        file1.application_process, file2.application_process,
        "ApplicationProcess mismatch"
    );

    // Compare Object Dictionaries
    assert_eq!(
        file1.object_dictionary.objects.len(),
        file2.object_dictionary.objects.len(),
        "Object count mismatch"
    );
    for (obj1, obj2) in file1
        .object_dictionary
        .objects
        .iter()
        .zip(file2.object_dictionary.objects.iter())
    {
        assert_eq!(obj1, obj2, "Object mismatch at index {}", obj1.index);
    }

    // 5. Final Full Compare
    assert_eq!(file1, file2, "XdcFile structs mismatch after round-trip");
}

/// Validates that the minimal "dynamic" XDD parses correctly.
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
    assert_eq!(flags_obj.data.as_deref(), Some("0x00000045"));
}