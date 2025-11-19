//! Handles the business logic of resolving values from a deserialized XDC/XDD model.
//!
//! This module is responsible for transforming the raw, schema-bound `model` structs
//! into the ergonomic, flattened `types` structs exposed by the crate.
//!
//! # Resolution Process
//! The XDC schema relies heavily on references (`uniqueIDRef`), templates, and inheritance.
//! This resolver implements a multi-pass approach:
//! 1.  **Templates:** Global templates are collected.
//! 2.  **Parameters:** Parameters in the `ApplicationProcess` are indexed.
//! 3.  **Data Types:** Custom data types are mapped.
//! 4.  **Object Dictionary:** Objects are resolved by looking up their values. If an object
//!     references a Parameter, and that Parameter references a Template, the value is
//!     resolved through the inheritance chain.

use crate::error::XdcError;
use crate::model;
use crate::model::Iso15745ProfileContainer;
use crate::model::app_layers::DataTypeName;
use crate::types;
use alloc::collections::BTreeMap;
use alloc::string::String;

mod app_process;
mod device_function;
pub mod device_manager;
mod header;
mod identity;
pub mod modular;
mod net_mgmt;
mod od;
pub mod utils;

/// Defines which value source to prioritize when resolving parameters and objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ValueMode {
    /// Prioritize `actualValue` attributes.
    ///
    /// This is used when loading **Configuration (.xdc)** files, where specific
    /// configuration values override the device defaults.
    Actual,
    /// Prioritize `defaultValue` attributes.
    ///
    /// This is used when loading **Device Description (.xdd)** files, where the
    /// standard default values of the device are required.
    Default,
}

/// Resolves the final `XdcFile` data from the raw deserialized container.
///
/// # Arguments
///
/// * `container` - The raw XML container parsed by `quick-xml`.
/// * `mode` - The resolution mode (`Actual` for XDC, `Default` for XDD).
///
/// # Returns
///
/// * `Result<types::XdcFile, XdcError>` - The fully resolved XDC file structure or a validation error.
pub(crate) fn resolve_data(
    container: Iso15745ProfileContainer,
    mode: ValueMode,
) -> Result<types::XdcFile, XdcError> {
    // EPSG 311 defines two specific profile types within the container.
    // We need to identify them to extract the relevant sections.

    // 1. Device Profile: Contains Identity, Device Manager, and Application Process.
    let device_profile = container
        .profile
        .iter()
        .find(|p| p.profile_body.device_identity.is_some());

    // 2. Communication Profile: Contains the Object Dictionary and Network Management.
    let comm_profile = container
        .profile
        .iter()
        .find(|p| p.profile_body.application_layers.is_some())
        .ok_or(XdcError::MissingElement {
            element: "Profile containing ApplicationLayers",
        })?;

    let device_profile_body = device_profile.map(|p| &p.profile_body);
    let comm_profile_body = &comm_profile.profile_body;

    // --- Pass 1: Build Template Map ---
    // We collect templates first because parameters refer to them.
    // The map stores: Template UniqueID -> Value Definition
    let mut template_map: BTreeMap<&String, &model::app_process::Value> = BTreeMap::new();

    let app_process = device_profile_body.and_then(|b| b.application_process.as_ref());

    if let Some(app_process) = app_process {
        if let Some(template_list) = &app_process.template_list {
            for template in &template_list.parameter_template {
                // Determine the value to cache based on the parsing mode.
                // Even in templates, an 'actualValue' might define a preset configuration.
                let value = match mode {
                    ValueMode::Actual => template
                        .actual_value
                        .as_ref()
                        .or(template.default_value.as_ref()),
                    ValueMode::Default => template
                        .default_value
                        .as_ref()
                        .or(template.actual_value.as_ref()),
                };

                if let Some(val) = value {
                    template_map.insert(&template.unique_id, val);
                }
            }
        }
    }

    // --- Pass 2: Build Parameter Map ---
    // We collect parameters next because Objects in the OD refer to them via `uniqueIDRef`.
    // Map stores: Parameter UniqueID -> Parameter Definition
    let mut param_map: BTreeMap<&String, &model::app_process::Parameter> = BTreeMap::new();

    if let Some(app_process) = app_process {
        for param in &app_process.parameter_list.parameter {
            // We store the reference to the parameter struct.
            // Value resolution (looking up the value inside the param or its template)
            // happens lazily during the Object Dictionary resolution phase.
            param_map.insert(&param.unique_id, param);
        }
    }

    // --- Pass 3: Build Data Type Map ---
    // Map stores: DataType ID (e.g. "0006") -> DataTypeName (e.g. Unsigned16)
    let app_layers =
        comm_profile_body
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

    // --- Pass 4: Resolve Public Types ---
    // Now that we have the maps, we can resolve the actual structures.

    // The header is mandatory in both profiles. By convention, we prefer the Device Profile header
    // if available, as it usually contains the device-specific versioning.
    let header_model = device_profile
        .map(|p| &p.profile_header)
        .unwrap_or(&comm_profile.profile_header);

    let header = header::resolve_header(header_model)?;

    let identity = device_profile_body
        .and_then(|b| b.device_identity.as_ref())
        .map(identity::resolve_identity)
        .transpose()?
        .unwrap_or_default();

    let device_function = device_profile_body
        .map(|b| device_function::resolve_device_function(&b.device_function))
        .transpose()?
        .unwrap_or_default();

    let device_manager = device_profile_body
        .and_then(|b| b.device_manager.as_ref())
        .map(device_manager::resolve_device_manager)
        .transpose()?;

    let network_management = comm_profile_body
        .network_management
        .as_ref()
        .map(net_mgmt::resolve_network_management)
        .transpose()?;

    let application_process_resolved = app_process
        .map(app_process::resolve_application_process)
        .transpose()?;

    let module_management_comm = app_layers
        .module_management
        .as_ref()
        .map(modular::resolve_module_management_comm)
        .transpose()?;

    // Resolve the Object Dictionary.
    // This requires the maps created in Passes 1, 2, and 3 to resolve references.
    let object_dictionary =
        od::resolve_object_dictionary(app_layers, &param_map, &template_map, &type_map, mode)?;

    // Assemble the final structure.
    Ok(types::XdcFile {
        header,
        identity,
        device_function,
        device_manager,
        network_management,
        application_process: application_process_resolved,
        object_dictionary,
        module_management_comm,
    })
}
