// crates/powerlink-rs-xdc/src/resolver/mod.rs

//! Handles the business logic of resolving values from a deserialized XDC/XDD model.
//!
//! This module contains the main `resolve_data` orchestrator and sub-modules
//! for handling specific parts of the XDC/XDD.

use crate::error::XdcError;
use crate::model;
use crate::model::app_layers::DataTypeName;
use crate::model::Iso15745ProfileContainer;
use crate::types;
use alloc::collections::BTreeMap;
use alloc::string::String;

// --- Sub-modules ---

mod app_process; // Add the new module
mod header;
mod identity;
mod net_mgmt;
mod od;
mod utils;

/// Defines which value to prioritize when resolving the Object Dictionary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ValueMode {
    /// Prioritize `actualValue` (for XDC files).
    Actual,
    /// Prioritize `defaultValue` (for XDD files).
    Default,
}

/// Resolves the final `XdcFile` data from the raw deserialized container.
/// This function contains all the logic for mapping the internal `model`
/// to the public, ergonomic `types`.
pub(crate) fn resolve_data(
    container: Iso15745ProfileContainer,
    mode: ValueMode,
) -> Result<types::XdcFile, XdcError> {
    // Find the distinct profile bodies.
    
    // The Device Profile contains Identity and ApplicationProcess.
    let device_profile = container
        .profile
        .iter()
        .find(|p| p.profile_body.device_identity.is_some());

    // The Comm Profile contains ApplicationLayers and NetworkManagement.
    let comm_profile = container
        .profile
        .iter()
        .find(|p| p.profile_body.application_layers.is_some())
        .ok_or(XdcError::MissingElement {
            element: "Profile containing ApplicationLayers",
        })?;
    
    // Get the bodies from the profiles
    let device_profile_body = device_profile.map(|p| &p.profile_body);
    let comm_profile_body = &comm_profile.profile_body;

    // --- Pass 1: Build Template Map ---
    // Stores a map of template uniqueID -> &Value element
    let mut template_map: BTreeMap<&String, &model::app_process::Value> = BTreeMap::new();

    // ApplicationProcess is in the Device Profile
    let app_process = device_profile_body.and_then(|b| b.application_process.as_ref());

    if let Some(app_process) = app_process {
        if let Some(template_list) = &app_process.template_list {
            for template in &template_list.parameter_template {
                // The value is chosen based on the parser's mode
                let value = match mode {
                    ValueMode::Actual => template.actual_value.as_ref().or(template.default_value.as_ref()),
                    ValueMode::Default => template.default_value.as_ref().or(template.actual_value.as_ref()),
                };

                if let Some(val) = value {
                    template_map.insert(&template.unique_id, val);
                }
            }
        }
    }
    // --- End of Pass 1 ---

    // --- Pass 2: Build Parameter Map (with template resolution) ---
    // Stores a map of parameter uniqueID -> &Parameter element
    let mut param_map: BTreeMap<&String, &model::app_process::Parameter> = BTreeMap::new();

    if let Some(app_process) = app_process {
        // This is the line that caused the error.
        // `parameter_list` is NOT an Option, so we remove `if let Some(...)`
        for param in &app_process.parameter_list.parameter {
            // Just store a reference to the parameter itself.
            // Value resolution will happen in Pass 3.
            param_map.insert(&param.unique_id, param);
        }
    }
    // --- End of Pass 2 ---

    // --- Pass 2.5: Build Data Type Map ---
    let app_layers = comm_profile_body
        .application_layers
        .as_ref()
        .ok_or(XdcError::MissingElement {
            element: "ApplicationLayers",
        })?;

    let mut type_map: BTreeMap<String, DataTypeName> = BTreeMap::new();
    if let Some(data_type_list) = &app_layers.data_type_list {
        for def_type in &data_type_list.def_type {
            type_map.insert(def_type.data_type.clone(), def_type.type_name);
        }
    }
    // --- End of Pass 2.5 ---

    // 4. --- Pass 3: Resolve all public-facing types ---

    // The header is mandatory in both profiles; prefer the device one.
    let header_model = device_profile
        .map(|p| &p.profile_header)
        .unwrap_or(&comm_profile.profile_header);
    
    let header = header::resolve_header(header_model)?;
    
    let identity = device_profile_body
        .and_then(|b| b.device_identity.as_ref())
        .map(identity::resolve_identity)
        .transpose()?
        .unwrap_or_default();

    let network_management = comm_profile_body
        .network_management
        .as_ref()
        .map(net_mgmt::resolve_network_management)
        .transpose()?;

    // Resolve the ApplicationProcess (new)
    let application_process = app_process
        .map(app_process::resolve_application_process)
        .transpose()?;

    let object_dictionary =
        od::resolve_object_dictionary(app_layers, &param_map, &template_map, &type_map, mode)?;
    
    // 5. --- Assemble final XdcFile ---
    Ok(types::XdcFile {
        header,
        identity,
        network_management,
        application_process, // Add the resolved data
        object_dictionary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        app_layers::{ApplicationLayers, Object, ObjectList},
        app_process::{ApplicationProcess, Parameter, ParameterDataType, ParameterList, Value},
        common::ReadOnlyString,
        header::ProfileHeader,
        identity::DeviceIdentity,
        net_mgmt::{GeneralFeatures, NetworkManagement},
        Iso15745Profile, ProfileBody,
    };
    use alloc::{string::ToString, vec, vec::Vec};

    /// Helper to create a minimal, valid `model::Iso15745ProfileContainer`
    fn create_test_container(
        include_device_profile: bool,
        include_comm_profile: bool,
    ) -> Iso15745ProfileContainer {
        let mut profiles = Vec::new();

        if include_device_profile {
            profiles.push(Iso15745Profile {
                profile_header: ProfileHeader {
                    profile_name: "Device Profile".to_string(),
                    ..Default::default()
                },
                profile_body: ProfileBody {
                    xsi_type: Some("ProfileBody_Device_Powerlink".into()),
                    device_identity: Some(DeviceIdentity {
                        vendor_name: ReadOnlyString {
                            value: "TestVendor".to_string(),
                            ..Default::default()
                        },
                        product_name: ReadOnlyString {
                            value: "TestProduct".to_string(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                    application_process: Some(ApplicationProcess {
                        parameter_list: ParameterList {
                            parameter: vec![Parameter {
                                unique_id: "param1".to_string(),
                                access: Some(model::app_process::ParameterAccess::ReadWrite),
                                support: Some(model::app_process::ParameterSupport::Mandatory),
                                persistent: true,
                                labels: Default::default(),
                                data_type: ParameterDataType::USINT,
                                default_value: Some(Value {
                                    value: "0x12".to_string(),
                                    ..Default::default()
                                }),
                                ..Default::default()
                            }],
                        },
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            });
        }

        if include_comm_profile {
            profiles.push(Iso15745Profile {
                profile_header: ProfileHeader {
                    profile_name: "Comm Profile".to_string(),
                    ..Default::default()
                },
                profile_body: ProfileBody {
                    xsi_type: Some("ProfileBody_CommunicationNetwork_Powerlink".into()),
                    application_layers: Some(ApplicationLayers {
                        object_list: ObjectList {
                            object: vec![Object {
                                index: "1000".to_string(),
                                name: "Device Type".to_string(),
                                object_type: "7".to_string(),
                                unique_id_ref: Some("param1".to_string()),
                                ..Default::default()
                            }],
                        },
                        ..Default::default()
                    }),
                    network_management: Some(NetworkManagement {
                        general_features: GeneralFeatures {
                            dll_feature_mn: false,
                            nmt_boot_time_not_active: "0".to_string(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            });
        }

        Iso15745ProfileContainer {
            profile: profiles,
            ..Default::default()
        }
    }

    #[test]
    fn test_resolve_data_happy_path() {
        let container = create_test_container(true, true);
        let result = resolve_data(container, ValueMode::Default);
        assert!(result.is_ok());
        let xdc_file = result.unwrap();

        // Check that Device Profile data was resolved
        assert_eq!(xdc_file.identity.vendor_name, "TestVendor");
        assert!(xdc_file.application_process.is_some());

        // Check that Comm Profile data was resolved
        assert_eq!(xdc_file.object_dictionary.objects.len(), 1);
        let obj = &xdc_file.object_dictionary.objects[0];
        assert_eq!(obj.index, 0x1000);

        // Check that OD resolver correctly used the param_map
        assert_eq!(obj.support, Some(types::ParameterSupport::Mandatory));
        assert_eq!(obj.persistent, true);
        assert_eq!(
            obj.access_type,
            Some(types::ParameterAccess::ReadWrite)
        );
        assert_eq!(obj.data.as_deref(), Some(&[0x12u8] as &[u8]));
    }

    #[test]
    fn test_resolve_data_missing_comm_profile() {
        let container = create_test_container(true, false);
        let result = resolve_data(container, ValueMode::Default);

        // A comm profile (with ApplicationLayers) is mandatory
        assert!(matches!(
            result,
            Err(XdcError::MissingElement {
                element: "Profile containing ApplicationLayers"
            })
        ));
    }

    #[test]
    fn test_resolve_data_missing_device_profile() {
        let container = create_test_container(false, true);
        let result = resolve_data(container, ValueMode::Default);
        assert!(result.is_ok());
        let xdc_file = result.unwrap();

        // Header should fall back to the Comm Profile's header
        assert_eq!(xdc_file.header.name, "Comm Profile");

        // Identity should be default (empty)
        assert_eq!(xdc_file.identity.vendor_name, "");
        assert_eq!(xdc_file.identity.vendor_id, 0);

        // ApplicationProcess should be None
        assert!(xdc_file.application_process.is_none());

        // OD should be resolved, but attributes will be the defaults from the <Object>
        // (since param_map is empty)
        assert_eq!(xdc_file.object_dictionary.objects.len(), 1);
        let obj = &xdc_file.object_dictionary.objects[0];
        assert_eq!(obj.index, 0x1000);
        assert_eq!(obj.support, None);
        assert_eq!(obj.persistent, false);
        assert_eq!(obj.access_type, None);
        assert_eq!(obj.data, None); // No value, because param_map was empty
    }
}