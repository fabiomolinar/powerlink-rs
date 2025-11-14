// crates/powerlink-rs-xdc/src/builder.rs

use crate::error::XdcError;
use crate::model;
use crate::types::XdcFile;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::fmt::Write;
use serde::Serialize;

/// Serializes `XdcFile` data into a standard XDC XML `String`.
///
/// This function converts the high-level `XdcFile` struct into the internal
/// `serde` model and then uses `quick-xml` to generate the XML string.
///
/// # Arguments
/// * `file` - The `XdcFile` data to serialize.
///
/// # Errors
/// Returns an `XdcError` if serialization fails.
pub fn save_xdc_to_string(file: &XdcFile) -> Result<String, XdcError> {
    // 1. Convert Identity to Device Profile
    let device_profile = build_device_profile(file);

    // 2. Convert Data to Communication Profile
    let comm_profile = build_comm_profile(file)?;

    // 3. Wrap in Container
    let container = model::Iso15745ProfileContainer {
        profile: vec![device_profile, comm_profile],
        ..Default::default() // Uses default xmlns attributes from model.rs
    };

    // 4. Serialize
    // Create a String buffer. String implements core::fmt::Write.
    let mut buffer = String::new();
    // We must write the XML declaration manually
    write!(&mut buffer, "{}", "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n")?;
    
    let mut serializer = quick_xml::se::Serializer::new(&mut buffer);
    serializer.indent(' ', 2); // Optional: Prettify the output

    container.serialize(serializer)?;

    // The buffer is already a String, no conversion needed.
    Ok(buffer)
}

fn build_device_profile(file: &XdcFile) -> model::Iso15745Profile {
    let identity = &file.identity;
    
    let versions: Vec<model::Version> = identity.versions.iter().map(|v| {
        model::Version {
            version_type: v.version_type.clone(),
            value: v.value.clone(),
        }
    }).collect();

    let device_identity = model::DeviceIdentity {
        vendor_name: identity.vendor_name.clone(),
        vendor_id: Some(format!("0x{:08X}", identity.vendor_id)),
        product_name: identity.product_name.clone(),
        product_id: Some(format!("0x{:08X}", identity.product_id)),
        version: versions,
    };

    model::Iso15745Profile {
        profile_header: model::ProfileHeader::default(),
        profile_body: model::ProfileBody {
            xsi_type: Some("ProfileBody_Device_Powerlink".into()),
            application_layers: None,
            device_identity: Some(device_identity),
            application_process: None,
            network_management: None,
        },
    }
}

fn build_comm_profile(file: &XdcFile) -> Result<model::Iso15745Profile, XdcError> {
    // Group CfmObjects by their index to build Object nesting
    let mut object_map: BTreeMap<u16, Vec<model::SubObject>> = BTreeMap::new();

    // Access the correct field `file.data.objects`
    for cfm_obj in &file.data.objects {
        // FIX: Add all missing fields
        let sub_object = model::SubObject {
            sub_index: format_hex_u8(cfm_obj.sub_index),
            name: "Sub".into(), // Required
            object_type: "7".into(), // Required (VAR)
            data_type: None,
            low_limit: None,
            high_limit: None,
            access_type: None,
            default_value: None, // XDC uses actualValue
            actual_value: Some(format_hex_string(&cfm_obj.data)?),
            denotation: None,
            pdo_mapping: None,
            obj_flags: None,
            unique_id_ref: None,
        };
        object_map.entry(cfm_obj.index).or_default().push(sub_object);
    }

    let mut objects = Vec::new();
    for (index, mut sub_objects) in object_map {
        // Sort sub-objects by sub-index for cleaner XML
        sub_objects.sort_by(|a, b| a.sub_index.cmp(&b.sub_index));

        // Add the NumberOfEntries (subIndex 00) if not present (heuristic)
        // NOTE: The count is sub_objects.len().
        // FIX: Add all missing fields
        let count_so = model::SubObject {
            sub_index: "00".into(),
            name: "NumberOfEntries".into(), // Required
            object_type: "7".into(), // Required (VAR)
            data_type: None,
            low_limit: None,
            high_limit: None,
            access_type: None,
            default_value: None,
            actual_value: Some(format!("{}", sub_objects.len())),
            denotation: None,
            pdo_mapping: None,
            obj_flags: None,
            unique_id_ref: None,
        };
        // We insert at the beginning
        sub_objects.insert(0, count_so);

        // FIX: Add all missing fields
        let object = model::Object {
            index: format_hex_u16(index),
            name: "Obj".into(), // Required
            // Per spec 7.5.4.4.1, objectType 9 is VAR_ARRAY
            object_type: "9".into(), // Required
            data_type: None,
            low_limit: None,
            high_limit: None,
            access_type: None,
            default_value: None,
            actual_value: None, // Value is on SubObjects for RECORD
            denotation: None,
            pdo_mapping: None,
            obj_flags: None,
            unique_id_ref: None,
            sub_object: sub_objects,
        };
        objects.push(object);
    }

    let app_layers = model::ApplicationLayers {
        object_list: model::ObjectList {
            object: objects,
        },
        data_type_list: None, // XDC files typically don't generate this
    };

    Ok(model::Iso15745Profile {
        profile_header: model::ProfileHeader::default(),
        profile_body: model::ProfileBody {
            xsi_type: Some("ProfileBody_CommunicationNetwork_Powerlink".into()),
            application_layers: Some(app_layers),
            device_identity: None,
            application_process: None,
            network_management: None,
        },
    })
}

// --- Helper Functions ---

fn format_hex_u16(val: u16) -> String {
    format!("{:04X}", val)
}

fn format_hex_u8(val: u8) -> String {
    format!("{:02X}", val)
}

fn format_hex_string(data: &[u8]) -> Result<String, XdcError> {
    let mut s = String::with_capacity(2 + data.len() * 2);
    s.push_str("0x");
    // This write! macro writes to a String, which implements core::fmt::Write.
    // The `?` operator will correctly convert a `core::fmt::Error` into
    // an `XdcError::FmtError` (via the From<fmt::Error> impl in src/error.rs).
    for &byte in data {
        write!(&mut s, "{:02X}", byte)?;
    }
    Ok(s)
}