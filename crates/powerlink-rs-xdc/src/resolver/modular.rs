// crates/powerlink-rs-xdc/src/resolver/modular.rs

//! Handles resolving modular device structs from the model to public types.

use crate::error::XdcError;
use crate::model;
use crate::parser::{parse_hex_u16, parse_hex_u8};
use crate::types;
use alloc::string::ToString;
use alloc::vec::Vec;
use crate::resolver::utils; // Use the OD utils for mapping

/// Helper to resolve a `<fileList>` into a `Vec<String>`.
fn resolve_file_list(model: &model::modular::FileList) -> Result<Vec<String>, XdcError> {
    Ok(model.file.iter().map(|f| f.uri.clone()).collect())
}

/// Helper to resolve a `<connectedModule>`.
fn resolve_connected_module(
    model: &model::modular::ConnectedModule,
) -> Result<types::ConnectedModule, XdcError> {
    let position = model.position.parse::<u32>().map_err(|_| {
        XdcError::InvalidAttributeFormat {
            attribute: "connectedModule @position",
        }
    })?;

    let address = model
        .address
        .as_ref()
        .map(|a| a.parse::<u32>())
        .transpose()
        .map_err(|_| XdcError::InvalidAttributeFormat {
            attribute: "connectedModule @address",
        })?;

    Ok(types::ConnectedModule {
        child_id_ref: model.child_id_ref.clone(),
        position,
        address,
    })
}

/// Helper to resolve a `<connectedModuleList>`.
fn resolve_connected_module_list(
    model: &model::modular::ConnectedModuleList,
) -> Result<Vec<types::ConnectedModule>, XdcError> {
    model
        .connected_module
        .iter()
        .map(resolve_connected_module)
        .collect()
}

/// Helper to resolve an `<interface>` from the Device profile.
fn resolve_interface_device(
    model: &model::modular::InterfaceDevice,
) -> Result<types::InterfaceDevice, XdcError> {
    let file_list = resolve_file_list(&model.file_list)?;

    let connected_modules = model
        .connected_module_list
        .as_ref()
        .map(resolve_connected_module_list)
        .transpose()?
        .unwrap_or_default();

    let max_modules = model.max_modules.parse::<u32>().map_err(|_| {
        XdcError::InvalidAttributeFormat {
            attribute: "interface @maxModules",
        }
    })?;

    let module_addressing = match model.module_addressing {
        model::modular::ModuleAddressingHead::Manual => "manual".to_string(),
        model::modular::ModuleAddressingHead::Position => "position".to_string(),
    };

    Ok(types::InterfaceDevice {
        unique_id: model.unique_id.clone(),
        interface_type: model.interface_type.clone(),
        max_modules,
        module_addressing,
        file_list,
        connected_modules,
    })
}

/// Helper to resolve an `<interfaceList>` from the Device profile.
fn resolve_interface_list_device(
    model: &model::modular::InterfaceListDevice,
) -> Result<Vec<types::InterfaceDevice>, XdcError> {
    model
        .interface
        .iter()
        .map(resolve_interface_device)
        .collect()
}

/// Helper to resolve a `<moduleInterface>`.
fn resolve_module_interface(
    model: &model::modular::ModuleInterface,
) -> Result<types::ModuleInterface, XdcError> {
    let module_addressing = match model.module_addressing {
        model::modular::ModuleAddressingChild::Manual => "manual".to_string(),
        model::modular::ModuleAddressingChild::Position => "position".to_string(),
        model::modular::ModuleAddressingChild::Next => "next".to_string(),
    };

    Ok(types::ModuleInterface {
        child_id: model.child_id.clone(),
        interface_type: model.interface_type.clone(),
        module_addressing,
    })
}

/// Resolves `<moduleManagement>` from the Device profile.
pub(super) fn resolve_module_management_device(
    model: &model::modular::ModuleManagementDevice,
) -> Result<types::ModuleManagementDevice, XdcError> {
    let interfaces = resolve_interface_list_device(&model.interface_list)?;

    let module_interface = model
        .module_interface
        .as_ref()
        .map(resolve_module_interface)
        .transpose()?;

    Ok(types::ModuleManagementDevice {
        interfaces,
        module_interface,
    })
}

/// Helper to resolve a `<range>`.
fn resolve_range(model: &model::modular::Range) -> Result<types::Range, XdcError> {
    let base_index = parse_hex_u16(&model.base_index)?;
    let max_index = model
        .max_index
        .as_ref()
        .map(|idx| parse_hex_u16(idx))
        .transpose()?;
    let max_sub_index = parse_hex_u8(&model.max_sub_index)?;

    let sort_mode = match model.sort_mode {
        model::modular::SortMode::Index => "index".to_string(),
        model::modular::SortMode::Subindex => "subindex".to_string(),
    };

    let sort_number = match model.sort_number {
        model::modular::AddressingAttribute::Continuous => "continuous".to_string(),
        model::modular::AddressingAttribute::Address => "address".to_string(),
    };

    let sort_step = model
        .sort_step
        .as_ref()
        .map(|s| s.parse::<u32>())
        .transpose()
        .map_err(|_| XdcError::InvalidAttributeFormat {
            attribute: "range @sortStep",
        })?;

    Ok(types::Range {
        name: model.name.clone(),
        base_index,
        max_index,
        max_sub_index,
        sort_mode,
        sort_number,
        sort_step,
        pdo_mapping: model.pdo_mapping.map(utils::map_pdo_mapping),
    })
}

/// Helper to resolve a `<rangeList>`.
fn resolve_range_list(
    model: &model::modular::RangeList,
) -> Result<Vec<types::Range>, XdcError> {
    model.range.iter().map(resolve_range).collect()
}

/// Helper to resolve an `<interface>` from the Communication profile.
fn resolve_interface_comm(
    model: &model::modular::InterfaceComm,
) -> Result<types::InterfaceComm, XdcError> {
    let ranges = resolve_range_list(&model.range_list)?;
    Ok(types::InterfaceComm {
        unique_id_ref: model.unique_id_ref.clone(),
        ranges,
    })
}

/// Helper to resolve an `<interfaceList>` from the Communication profile.
fn resolve_interface_list_comm(
    model: &model::modular::InterfaceListComm,
) -> Result<Vec<types::InterfaceComm>, XdcError> {
    model.interface.iter().map(resolve_interface_comm).collect()
}

/// Resolves `<moduleManagement>` from the Communication profile.
pub(super) fn resolve_module_management_comm(
    model: &model::modular::ModuleManagementComm,
) -> Result<types::ModuleManagementComm, XdcError> {
    let interfaces = resolve_interface_list_comm(&model.interface_list)?;
    Ok(types::ModuleManagementComm { interfaces })
}