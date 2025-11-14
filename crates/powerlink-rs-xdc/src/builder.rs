// crates/powerlink-rs-xdc/src/builder.rs

use crate::error::XdcError;
use crate::model;
use crate::types; // Use the main types module
use crate::types::XdcFile;
use alloc::format;
use alloc::vec;
use alloc::string::String;
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
    let device_profile = build_device_profile(&file.header, &file.identity);

    // 2. Convert Data to Communication Profile
    let comm_profile = build_comm_profile(&file.header, &file.object_dictionary)?;

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

/// Helper to build the `model::ProfileHeader` from the `types::ProfileHeader`.
fn build_model_header(header: &types::ProfileHeader) -> model::ProfileHeader {
    model::ProfileHeader {
        profile_identification: header.identification.clone(),
        profile_revision: header.revision.clone(),
        profile_name: header.name.clone(),
        profile_source: header.source.clone(),
        profile_date: header.date.clone(),
        ..Default::default() // Fills in ProfileClassID, etc.
    }
}

/// Builds the Device Profile model from the public types.
fn build_device_profile(
    header: &types::ProfileHeader,
    identity: &types::Identity,
) -> model::Iso15745Profile {
    let model_header = build_model_header(header);

    let versions: Vec<model::Version> = identity
        .versions
        .iter()
        .map(|v| model::Version {
            version_type: v.version_type.clone(),
            value: v.value.clone(),
        })
        .collect();

    let device_identity = model::DeviceIdentity {
        vendor_name: identity.vendor_name.clone(),
        vendor_id: Some(format!("0x{:08X}", identity.vendor_id)),
        product_name: identity.product_name.clone(),
        product_id: Some(format!("0x{:08X}", identity.product_id)),
        version: versions,
    };

    model::Iso15745Profile {
        profile_header: model_header,
        profile_body: model::ProfileBody {
            xsi_type: Some("ProfileBody_Device_Powerlink".into()),
            application_layers: None,
            device_identity: Some(device_identity),
            application_process: None,
            network_management: None,
        },
    }
}

/// Builds the Communication Profile model from the public Object Dictionary.
fn build_comm_profile(
    header: &types::ProfileHeader,
    od: &types::ObjectDictionary,
) -> Result<model::Iso15745Profile, XdcError> {
    let model_header = build_model_header(header);

    let mut model_objects = Vec::new();

    // Iterate over the rich public `Object` type
    for obj in &od.objects {
        // Convert public `SubObject`s back to `model::SubObject`s
        let model_sub_objects = obj
            .sub_objects
            .iter()
            .map(|sub_obj| {
                // Serialize the data back into a hex string
                let actual_value = sub_obj
                    .data
                    .as_ref()
                    .map(|d| format_hex_string(d))
                    .transpose()?;

                Ok(model::SubObject {
                    sub_index: format_hex_u8(sub_obj.sub_index),
                    name: sub_obj.name.clone(),
                    object_type: sub_obj.object_type.clone(),
                    actual_value,
                    // Fill in required fields from model
                    data_type: None,
                    low_limit: None,
                    high_limit: None,
                    access_type: None,
                    default_value: None,
                    denotation: None,
                    pdo_mapping: None,
                    obj_flags: None,
                    unique_id_ref: None,
                })
            })
            .collect::<Result<Vec<_>, XdcError>>()?;
        
        // Handle the value for VAR objects (value is on the object itself)
        let object_actual_value = obj.data.as_ref().map(|d| format_hex_string(d)).transpose()?;

        let model_object = model::Object {
            index: format_hex_u16(obj.index),
            name: obj.name.clone(),
            object_type: obj.object_type.clone(),
            actual_value: object_actual_value,
            sub_object: model_sub_objects,
            // Fill in required fields from model
            data_type: None,
            low_limit: None,
            high_limit: None,
            access_type: None,
            default_value: None,
            denotation: None,
            pdo_mapping: None,
            obj_flags: None,
            unique_id_ref: None,
        };
        model_objects.push(model_object);
    }

    let app_layers = model::ApplicationLayers {
        object_list: model::ObjectList {
            object: model_objects,
        },
        data_type_list: None, // XDC files typically don't generate this
    };

    Ok(model::Iso15745Profile {
        profile_header: model_header,
        profile_body: model::ProfileBody {
            xsi_type: Some("ProfileBody_CommunicationNetwork_Powerlink".into()),
            application_layers: Some(app_layers),
            device_identity: None,
            application_process: None,
            network_management: None, // We are not serializing this yet
        },
    })
}

// --- Helper Functions ---

/// Formats a u16 as a 4-digit hex string (e.g., "1F80").
fn format_hex_u16(val: u16) -> String {
    format!("{:04X}", val)
}

/// Formats a u8 as a 2-digit hex string (e.g., "0A").
fn format_hex_u8(val: u8) -> String {
    format!("{:02X}", val)
}

/// Formats a byte slice into a "0x..." hex string.
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