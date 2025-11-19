//! Provides functionality to serialize `XdcFile` structs back into XDC-compliant XML strings.
//!
//! This module implements the conversion from the user-friendly public `types`
//! back to the internal `model` structs required by `quick-xml` for correct serialization
//! according to the EPSG DS 311 schema.

pub mod app_process;
pub mod device_function;
pub mod device_manager;
pub mod modular;
pub mod net_mgmt;

use crate::error::XdcError;
use crate::model;
use crate::types;
use crate::types::XdcFile;
use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;
use core::fmt::Write;
use serde::Serialize;

use crate::model::app_layers::{Object, ObjectAccessType, ObjectList, ObjectPdoMapping, SubObject};
use crate::model::common::ReadOnlyString;
use crate::model::header::ProfileHeader;
use crate::model::identity::{DeviceIdentity, Version};

/// Serializes an `XdcFile` data structure into a standard XDC XML string.
///
/// This function generates a complete XML document, including the standard header
/// and the ISO 15745 container structure. It handles both the Device Profile
/// (identity, functions) and the Communication Profile (Object Dictionary, Network Management).
///
/// # Arguments
/// * `file` - The `XdcFile` structure containing the device configuration.
///
/// # Returns
/// * `Result<String, XdcError>` - The formatted XML string or a serialization error.
pub fn save_xdc_to_string(file: &XdcFile) -> Result<String, XdcError> {
    // 1. Convert Identity, DeviceManager, and AppProcess to Device Profile
    let device_profile = build_device_profile(
        &file.header,
        &file.identity,
        &file.device_function,
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
        ..Default::default()
    };

    // 4. Serialize to string
    let mut buffer = String::new();
    write!(
        &mut buffer,
        "{}",
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n"
    )?;

    let mut serializer = quick_xml::se::Serializer::new(&mut buffer);
    serializer.indent(' ', 2);

    container.serialize(serializer)?;
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
        ..Default::default()
    }
}

/// Constructs the internal `Iso15745Profile` model representing the Device Profile.
fn build_device_profile(
    header: &types::ProfileHeader,
    identity: &types::Identity,
    device_function: &[types::DeviceFunction],
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

    let device_identity =
        DeviceIdentity {
            vendor_name: ReadOnlyString {
                value: identity.vendor_name.clone(),
                ..Default::default()
            },
            vendor_id: Some(ReadOnlyString {
                value: format!("0x{:08X}", identity.vendor_id),
                ..Default::default()
            }),
            product_name: ReadOnlyString {
                value: identity.product_name.clone(),
                ..Default::default()
            },
            product_id: Some(ReadOnlyString {
                value: format!("0x{:X}", identity.product_id),
                ..Default::default()
            }),
            version: versions,
            vendor_text: identity
                .vendor_text
                .as_ref()
                .map(|t| model::common::AttributedGlabels {
                    items: vec![model::common::LabelChoice::Label(model::common::Label {
                        lang: "en".to_string(),
                        value: t.clone(),
                    })],
                    ..Default::default()
                }),
            device_family: identity.device_family.as_ref().map(|t| {
                model::common::AttributedGlabels {
                    items: vec![model::common::LabelChoice::Label(model::common::Label {
                        lang: "en".to_string(),
                        value: t.clone(),
                    })],
                    ..Default::default()
                }
            }),
            product_family: identity.product_family.as_ref().map(|t| ReadOnlyString {
                value: t.clone(),
                ..Default::default()
            }),
            product_text: identity.product_text.as_ref().map(|t| {
                model::common::AttributedGlabels {
                    items: vec![model::common::LabelChoice::Label(model::common::Label {
                        lang: "en".to_string(),
                        value: t.clone(),
                    })],
                    ..Default::default()
                }
            }),
            order_number: identity
                .order_number
                .iter()
                .map(|o| ReadOnlyString {
                    value: o.clone(),
                    ..Default::default()
                })
                .collect(),
            build_date: identity.build_date.clone(),
            specification_revision: identity.specification_revision.as_ref().map(|sr| {
                ReadOnlyString {
                    value: sr.clone(),
                    ..Default::default()
                }
            }),
            instance_name: identity
                .instance_name
                .as_ref()
                .map(|i| model::common::InstanceName {
                    value: i.clone(),
                    ..Default::default()
                }),
        };

    let model_device_function = device_function::build_model_device_function(device_function);
    let model_device_manager = device_manager.map(device_manager::build_model_device_manager);
    let model_application_process =
        application_process.map(app_process::build_model_application_process);

    model::Iso15745Profile {
        profile_header: model_header,
        profile_body: model::ProfileBody {
            xsi_type: Some("ProfileBody_Device_Powerlink".into()),
            application_layers: None,
            device_identity: Some(device_identity),
            device_function: model_device_function,
            device_manager: model_device_manager,
            application_process: model_application_process,
            network_management: None,
        },
    }
}

/// Constructs the internal `Iso15745Profile` model representing the Communication Profile.
fn build_comm_profile(
    header: &types::ProfileHeader,
    od: &types::ObjectDictionary,
    network_management: Option<&types::NetworkManagement>,
    module_management_comm: Option<&types::ModuleManagementComm>,
) -> Result<model::Iso15745Profile, XdcError> {
    let model_header = build_model_header(header);
    let mut model_objects = Vec::new();

    for obj in &od.objects {
        let model_sub_objects = obj
            .sub_objects
            .iter()
            .map(|sub_obj| {
                let actual_value = sub_obj
                    .data
                    .as_ref()
                    .map(|d| format_value_to_string(d.as_bytes(), sub_obj.data_type.as_deref()))
                    .transpose()?;

                Ok(SubObject {
                    sub_index: format_hex_u8(sub_obj.sub_index),
                    name: sub_obj.name.clone(),
                    object_type: sub_obj.object_type.clone(),
                    actual_value,
                    data_type: sub_obj.data_type.clone(),
                    low_limit: sub_obj.low_limit.clone(),
                    high_limit: sub_obj.high_limit.clone(),
                    access_type: sub_obj.access_type.map(map_access_type_to_model),
                    default_value: None,
                    denotation: None,
                    pdo_mapping: sub_obj.pdo_mapping.map(map_pdo_mapping_to_model),
                    obj_flags: sub_obj.obj_flags.clone(),
                    unique_id_ref: None,
                })
            })
            .collect::<Result<Vec<_>, XdcError>>()?;

        let object_actual_value = obj
            .data
            .as_ref()
            .map(|d| format_value_to_string(d.as_bytes(), obj.data_type.as_deref()))
            .transpose()?;

        let model_object = Object {
            index: format_hex_u16(obj.index),
            name: obj.name.clone(),
            object_type: obj.object_type.clone(),
            actual_value: object_actual_value,
            sub_object: model_sub_objects,
            data_type: obj.data_type.clone(),
            low_limit: obj.low_limit.clone(),
            high_limit: obj.high_limit.clone(),
            access_type: obj.access_type.map(map_access_type_to_model),
            default_value: None,
            denotation: None,
            pdo_mapping: obj.pdo_mapping.map(map_pdo_mapping_to_model),
            obj_flags: obj.obj_flags.clone(),
            unique_id_ref: None,
            range_selector: None,
        };
        model_objects.push(model_object);
    }

    let model_module_mgmt_comm =
        module_management_comm.map(modular::build_model_module_management_comm);

    let app_layers = model::app_layers::ApplicationLayers {
        object_list: ObjectList {
            object: model_objects,
        },
        data_type_list: None,
        module_management: model_module_mgmt_comm,
    };

    let model_network_management = network_management.map(net_mgmt::build_model_network_management);

    Ok(model::Iso15745Profile {
        profile_header: model_header,
        profile_body: model::ProfileBody {
            xsi_type: Some("ProfileBody_CommunicationNetwork_Powerlink".into()),
            application_layers: Some(app_layers),
            device_identity: None,
            device_manager: None,
            device_function: Vec::new(),
            application_process: None,
            network_management: model_network_management,
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

/// Helper to format a value into a string for serialization.
/// Currently assumes input bytes are already valid UTF-8 strings.
fn format_value_to_string(data: &[u8], _data_type_id: Option<&str>) -> Result<String, XdcError> {
    if let Ok(s) = core::str::from_utf8(data) {
        return Ok(s.to_string());
    }
    Err(XdcError::FmtError(core::fmt::Error))
}

fn map_access_type_to_model(public: types::ParameterAccess) -> ObjectAccessType {
    match public {
        types::ParameterAccess::ReadOnly => ObjectAccessType::ReadOnly,
        types::ParameterAccess::WriteOnly => ObjectAccessType::WriteOnly,
        types::ParameterAccess::ReadWrite => ObjectAccessType::ReadWrite,
        types::ParameterAccess::Constant => ObjectAccessType::Constant,
        types::ParameterAccess::ReadWriteInput => ObjectAccessType::ReadWrite,
        types::ParameterAccess::ReadWriteOutput => ObjectAccessType::ReadWrite,
        types::ParameterAccess::NoAccess => ObjectAccessType::ReadOnly,
    }
}

fn map_pdo_mapping_to_model(public: types::ObjectPdoMapping) -> ObjectPdoMapping {
    match public {
        types::ObjectPdoMapping::No => ObjectPdoMapping::No,
        types::ObjectPdoMapping::Default => ObjectPdoMapping::Default,
        types::ObjectPdoMapping::Optional => ObjectPdoMapping::Optional,
        types::ObjectPdoMapping::Tpdo => ObjectPdoMapping::Tpdo,
        types::ObjectPdoMapping::Rpdo => ObjectPdoMapping::Rpdo,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Iso15745ProfileContainer;
    use crate::types::{self, Object, ObjectDictionary, SubObject};
    use alloc::string::ToString;
    use alloc::vec;

    #[test]
    fn test_save_xdc_to_string() {
        let xdc_file = types::XdcFile {
            header: types::ProfileHeader {
                identification: "Test XDC".to_string(),
                revision: "1.0.0".to_string(),
                name: "My Test Device".to_string(),
                source: "powerlink-rs".to_string(),
                date: Some("2024-01-01".to_string()),
            },
            identity: types::Identity {
                vendor_name: "MyVendor".to_string(),
                vendor_id: 0x12345678,
                product_name: "MyProduct".to_string(),
                product_id: 0xABCD,
                versions: vec![types::Version {
                    version_type: "HW".to_string(),
                    value: "1.2".to_string(),
                }],
                ..Default::default()
            },
            object_dictionary: ObjectDictionary {
                objects: vec![
                    Object {
                        index: 0x1000,
                        name: "Device Type".to_string(),
                        object_type: "7".to_string(),
                        data_type: Some("0007".to_string()),
                        access_type: Some(types::ParameterAccess::Constant),
                        data: Some(String::from("0x91010F00")),
                        ..Default::default()
                    },
                    Object {
                        index: 0x1018,
                        name: "Identity".to_string(),
                        object_type: "9".to_string(),
                        sub_objects: vec![
                            SubObject {
                                sub_index: 0,
                                name: "Count".to_string(),
                                object_type: "7".to_string(),
                                data_type: Some("0005".to_string()),
                                access_type: Some(types::ParameterAccess::Constant),
                                data: Some(String::from("4")),
                                ..Default::default()
                            },
                            SubObject {
                                sub_index: 1,
                                name: "VendorID".to_string(),
                                object_type: "7".to_string(),
                                data_type: Some("0007".to_string()),
                                access_type: Some(types::ParameterAccess::Constant),
                                data: Some(String::from("0x78563412")),
                                ..Default::default()
                            },
                        ],
                        ..Default::default()
                    },
                ],
            },
            ..Default::default()
        };

        let xml_string = save_xdc_to_string(&xdc_file).unwrap();

        // Parse back to verify integrity
        let container: Iso15745ProfileContainer =
            quick_xml::de::from_str(&xml_string).expect("Serialized XML should be valid");

        assert_eq!(container.profile.len(), 2);

        let dev_profile = container.profile.get(0).unwrap();
        assert_eq!(dev_profile.profile_header.profile_name, "My Test Device");

        let identity = dev_profile.profile_body.device_identity.as_ref().unwrap();
        assert_eq!(identity.vendor_name.value, "MyVendor");
        assert_eq!(identity.vendor_id.as_ref().unwrap().value, "0x12345678");
        assert_eq!(identity.product_name.value, "MyProduct");
        assert_eq!(identity.product_id.as_ref().unwrap().value, "0xABCD");
        assert_eq!(identity.version[0].value, "1.2");

        let comm_profile = container.profile.get(1).unwrap();
        let app_layers = comm_profile
            .profile_body
            .application_layers
            .as_ref()
            .unwrap();
        let obj_list = &app_layers.object_list.object;
        assert_eq!(obj_list.len(), 2);
        assert_eq!(obj_list[0].index, "1000");
        assert_eq!(obj_list[0].name, "Device Type");
        assert_eq!(obj_list[0].actual_value, Some("0x91010F00".to_string()));
        assert_eq!(
            obj_list[0].access_type,
            Some(model::app_layers::ObjectAccessType::Constant)
        );
        assert_eq!(obj_list[1].index, "1018");
        assert_eq!(obj_list[1].sub_object.len(), 2);
        assert_eq!(obj_list[1].sub_object[1].name, "VendorID");
        assert_eq!(obj_list[1].sub_object[1].sub_index, "01");
        assert_eq!(
            obj_list[1].sub_object[1].actual_value,
            Some("0x78563412".to_string())
        );
    }

    #[test]
    fn test_map_access_type_to_model() {
        use crate::model::app_layers::ObjectAccessType as ModelAccess;
        use crate::types::ParameterAccess as PublicAccess;

        assert_eq!(
            map_access_type_to_model(PublicAccess::ReadOnly),
            ModelAccess::ReadOnly
        );
        assert_eq!(
            map_access_type_to_model(PublicAccess::WriteOnly),
            ModelAccess::WriteOnly
        );
        assert_eq!(
            map_access_type_to_model(PublicAccess::ReadWrite),
            ModelAccess::ReadWrite
        );
        assert_eq!(
            map_access_type_to_model(PublicAccess::Constant),
            ModelAccess::Constant
        );
        assert_eq!(
            map_access_type_to_model(PublicAccess::ReadWriteInput),
            ModelAccess::ReadWrite
        );
        assert_eq!(
            map_access_type_to_model(PublicAccess::ReadWriteOutput),
            ModelAccess::ReadWrite
        );
        assert_eq!(
            map_access_type_to_model(PublicAccess::NoAccess),
            ModelAccess::ReadOnly
        );
    }

    #[test]
    fn test_map_pdo_mapping_to_model() {
        use crate::model::app_layers::ObjectPdoMapping as ModelPdo;
        use crate::types::ObjectPdoMapping as PublicPdo;

        assert_eq!(map_pdo_mapping_to_model(PublicPdo::No), ModelPdo::No);
        assert_eq!(
            map_pdo_mapping_to_model(PublicPdo::Default),
            ModelPdo::Default
        );
        assert_eq!(
            map_pdo_mapping_to_model(PublicPdo::Optional),
            ModelPdo::Optional
        );
        assert_eq!(map_pdo_mapping_to_model(PublicPdo::Tpdo), ModelPdo::Tpdo);
        assert_eq!(map_pdo_mapping_to_model(PublicPdo::Rpdo), ModelPdo::Rpdo);
    }
}
