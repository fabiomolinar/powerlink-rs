// crates/powerlink-rs-xdc/src/builder/mod.rs

// Declare the new modules for serialization logic
pub mod app_process;
pub mod device_manager;
pub mod modular;
pub mod net_mgmt;

use crate::error::XdcError;
use crate::model;
use crate::types; // Use the main types module
use crate::types::XdcFile;
use alloc::format;
use alloc::string::ToString;
use alloc::vec;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;
use serde::Serialize;

// Import new model paths
// Use fully qualified paths
use crate::model::app_layers::{Object, ObjectAccessType, ObjectList, ObjectPdoMapping, SubObject};
use crate::model::common::ReadOnlyString;
use crate::model::header::ProfileHeader;
use crate::model::identity::{DeviceIdentity, Version};

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
    // 1. Convert Identity, DeviceManager, and AppProcess to Device Profile
    let device_profile = build_device_profile(
        &file.header,
        &file.identity,
        file.device_manager.as_ref(),
        file.application_process.as_ref(),
    );

    // 2. Convert OD, NetworkMgmt, and ModuleMgmtComm to Communication Profile
    let comm_profile = build_comm_profile(
        &file.header,
        &file.object_dictionary,
        file.network_management.as_ref(),
        file.module_management_comm.as_ref(),
    )?;

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
fn build_model_header(header: &types::ProfileHeader) -> ProfileHeader {
    model::header::ProfileHeader {
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
    device_manager: Option<&types::DeviceManager>,
    application_process: Option<&types::ApplicationProcess>,
) -> model::Iso15745Profile {
    let model_header = build_model_header(header);

    let versions: Vec<Version> = identity
        .versions
        .iter()
        .map(|v| Version {
            version_type: v.version_type.clone(),
            value: v.value.clone(),
            read_only: true,
        })
        .collect();

    let device_identity = DeviceIdentity {
        vendor_name: ReadOnlyString { value: identity.vendor_name.clone(), ..Default::default() },
        vendor_id: Some(ReadOnlyString { value: format!("0x{:08X}", identity.vendor_id), ..Default::default() }),
        product_name: ReadOnlyString { value: identity.product_name.clone(), ..Default::default() },
        product_id: Some(ReadOnlyString { value: format!("{:X}", identity.product_id), ..Default::default() }),
        version: versions,
        vendor_text: identity.vendor_text.as_ref().map(|t| model::common::AttributedGlabels {
            labels: model::common::Glabels {
                items: vec![model::common::LabelChoice::Label(model::common::Label {
                    lang: "en".to_string(),
                    value: t.clone(),
                })],
            },
            ..Default::default()
        }),
        device_family: identity.device_family.as_ref().map(|t| model::common::AttributedGlabels {
            labels: model::common::Glabels {
                items: vec![model::common::LabelChoice::Label(model::common::Label {
                    lang: "en".to_string(),
                    value: t.clone(),
                })],
            },
            ..Default::default()
        }),
        product_family: identity.product_family.as_ref().map(|t| ReadOnlyString {
            value: t.clone(),
            ..Default::default()
        }),
        product_text: identity.product_text.as_ref().map(|t| model::common::AttributedGlabels {
            labels: model::common::Glabels {
                items: vec![model::common::LabelChoice::Label(model::common::Label {
                    lang: "en".to_string(),
                    value: t.clone(),
                })],
            },
            ..Default::default()
        }),
        order_number: identity.order_number.iter().map(|o| ReadOnlyString {
            value: o.clone(),
            ..Default::default()
        }).collect(),
        build_date: identity.build_date.clone(),
        specification_revision: identity.specification_revision.as_ref().map(|sr| ReadOnlyString {
            value: sr.clone(),
            ..Default::default()
        }),
        instance_name: identity.instance_name.as_ref().map(|i| model::common::InstanceName {
            value: i.clone(),
            ..Default::default()
        }),
    };

    // Call builders for device_manager and application_process
    let model_device_manager = device_manager.map(device_manager::build_model_device_manager);
    let model_application_process =
        application_process.map(app_process::build_model_application_process);
    
    model::Iso15745Profile {
        profile_header: model_header,
        profile_body: model::ProfileBody {
            xsi_type: Some("ProfileBody_Device_Powerlink".into()),
            application_layers: None,
            device_identity: Some(device_identity),
            device_manager: model_device_manager,
            application_process: model_application_process,
            network_management: None,
        },
    }
}

/// Builds the Communication Profile model from the public Object Dictionary.
fn build_comm_profile(
    header: &types::ProfileHeader,
    od: &types::ObjectDictionary,
    network_management: Option<&types::NetworkManagement>,
    module_management_comm: Option<&types::ModuleManagementComm>,
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

                Ok(SubObject {
                    sub_index: format_hex_u8(sub_obj.sub_index),
                    name: sub_obj.name.clone(),
                    object_type: sub_obj.object_type.clone(),
                    actual_value,
                    // Fill in required fields from model
                    data_type: sub_obj.data_type.clone(), // Pass through
                    low_limit: sub_obj.low_limit.clone(), // Pass through
                    high_limit: sub_obj.high_limit.clone(), // Pass through
                    access_type: sub_obj.access_type.map(map_access_type_to_model), // Map back
                    default_value: None, // We only serialize actualValue for XDC
                    denotation: None,
                    pdo_mapping: sub_obj.pdo_mapping.map(map_pdo_mapping_to_model), // Map back
                    obj_flags: sub_obj.obj_flags.clone(), // Pass through
                    unique_id_ref: None, // Not supported in builder yet
                })
            })
            .collect::<Result<Vec<_>, XdcError>>()?;
        
        // Handle the value for VAR objects (value is on the object itself)
        let object_actual_value = obj.data.as_ref().map(|d| format_hex_string(d)).transpose()?;

        let model_object = Object {
            index: format_hex_u16(obj.index),
            name: obj.name.clone(),
            object_type: obj.object_type.clone(),
            actual_value: object_actual_value,
            sub_object: model_sub_objects,
            // Fill in required fields from model
            data_type: obj.data_type.clone(), // Pass through
            low_limit: obj.low_limit.clone(), // Pass through
            high_limit: obj.high_limit.clone(), // Pass through
            access_type: obj.access_type.map(map_access_type_to_model), // Map back
            default_value: None, // We only serialize actualValue for XDC
            denotation: None,
            pdo_mapping: obj.pdo_mapping.map(map_pdo_mapping_to_model), // Map back
            obj_flags: obj.obj_flags.clone(), // Pass through
            unique_id_ref: None, // Not supported in builder yet
            range_selector: None,
        };
        model_objects.push(model_object);
    }

    // Call builder for module_management_comm
    let model_module_mgmt_comm =
        module_management_comm.map(modular::build_model_module_management_comm);
    
    let app_layers = model::app_layers::ApplicationLayers {
        object_list: ObjectList {
            object: model_objects,
        },
        data_type_list: None, // XDC files typically don't generate this
        module_management: model_module_mgmt_comm,
    };
    
    // Call builder for network_management
    let model_network_management =
        network_management.map(net_mgmt::build_model_network_management);

    Ok(model::Iso15745Profile {
        profile_header: model_header,
        profile_body: model::ProfileBody {
            xsi_type: Some("ProfileBody_CommunicationNetwork_Powerlink".into()),
            application_layers: Some(app_layers),
            device_identity: None,
            device_manager: None,
            application_process: None,
            network_management: model_network_management,
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

/// Maps the public types enum back to the internal model enum.
// Fix: Use new public enum `types::ParameterAccess`
fn map_access_type_to_model(public: types::ParameterAccess) -> ObjectAccessType {
    match public {
        // Fix: Match on `types::ParameterAccess` variants
        types::ParameterAccess::ReadOnly => ObjectAccessType::ReadOnly,
        types::ParameterAccess::WriteOnly => ObjectAccessType::WriteOnly,
        types::ParameterAccess::ReadWrite => ObjectAccessType::ReadWrite,
        types::ParameterAccess::Constant => ObjectAccessType::Constant,
        // Map new variants to best-fit `ObjectAccessType`
        types::ParameterAccess::ReadWriteInput => ObjectAccessType::ReadWrite,
        types::ParameterAccess::ReadWriteOutput => ObjectAccessType::ReadWrite,
        types::ParameterAccess::NoAccess => ObjectAccessType::ReadOnly, // Or ReadOnly? `const`? No good match.
    }
}

/// Maps the public types enum back to the internal model enum.
fn map_pdo_mapping_to_model(public: types::ObjectPdoMapping) -> ObjectPdoMapping {
    match public {
        types::ObjectPdoMapping::No => ObjectPdoMapping::No,
        types::ObjectPdoMapping::Default => ObjectPdoMapping::Default,
        types::ObjectPdoMapping::Optional => ObjectPdoMapping::Optional,
        types::ObjectPdoMapping::Tpdo => ObjectPdoMapping::Tpdo,
        types::ObjectPdoMapping::Rpdo => ObjectPdoMapping::Rpdo,
    }
}