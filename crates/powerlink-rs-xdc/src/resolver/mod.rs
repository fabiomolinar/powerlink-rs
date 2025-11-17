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
mod device_function; // Added new module
mod header;
mod identity;
mod net_mgmt;
mod od;
mod utils;
pub mod device_manager; // Added new module
pub mod modular;        // Added new module

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

    // Resolve the DeviceFunction (New)
    let device_function = device_profile_body
        .map(|b| device_function::resolve_device_function(&b.device_function))
        .transpose()?
        .unwrap_or_default();

    // Resolve the DeviceManager (New)
    let device_manager = device_profile_body
        .and_then(|b| b.device_manager.as_ref())
        .map(device_manager::resolve_device_manager)
        .transpose()?;

    let network_management = comm_profile_body
        .network_management
        .as_ref()
        .map(net_mgmt::resolve_network_management)
        .transpose()?;

    // Resolve the ApplicationProcess (new)
    let application_process = app_process
        .map(app_process::resolve_application_process)
        .transpose()?;

    // Resolve the modular communication ranges (New)
    let module_management_comm = app_layers
        .module_management
        .as_ref()
        .map(modular::resolve_module_management_comm)
        .transpose()?;

    let object_dictionary =
        od::resolve_object_dictionary(app_layers, &param_map, &template_map, &type_map, mode)?;
    
    // 5. --- Assemble final XdcFile ---
    Ok(types::XdcFile {
        header,
        identity,
        device_function, // Add the resolved data
        device_manager, // Add the resolved data
        network_management,
        application_process, // Add the resolved data
        object_dictionary,
        module_management_comm, // Add the resolved data
    })
}