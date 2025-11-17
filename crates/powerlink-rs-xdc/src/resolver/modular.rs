// crates/powerlink-rs-xdc/src/resolver/modular.rs

//! Handles resolving modular device structs from the model to public types.

use crate::error::XdcError;
use crate::model;
use crate::parser::{parse_hex_u8, parse_hex_u16};
use crate::resolver::utils;
use crate::types;
use alloc::string::{String, ToString}; // Fix: Add String import
use alloc::vec::Vec; // Use the OD utils for mapping

/// Helper to resolve a `<fileList>` into a `Vec<String>`.
fn resolve_file_list(model: &model::modular::FileList) -> Result<Vec<String>, XdcError> {
    Ok(model.file.iter().map(|f| f.uri.clone()).collect())
}

/// Helper to resolve a `<connectedModule>`.
fn resolve_connected_module(
    model: &model::modular::ConnectedModule,
) -> Result<types::ConnectedModule, XdcError> {
    let position = model
        .position
        .parse::<u32>()
        .map_err(|_| XdcError::InvalidAttributeFormat {
            attribute: "connectedModule @position",
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

    let max_modules =
        model
            .max_modules
            .parse::<u32>()
            .map_err(|_| XdcError::InvalidAttributeFormat {
                attribute: "interface @maxModules",
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
fn resolve_range_list(model: &model::modular::RangeList) -> Result<Vec<types::Range>, XdcError> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::app_layers::ObjectPdoMapping;
    use crate::model::modular as model_mod;
    use crate::types;
    use alloc::string::ToString;
    use alloc::vec;

    // --- resolve_module_management_device tests ---

    #[test]
    fn test_resolve_connected_module() {
        let model_cm = model_mod::ConnectedModule {
            child_id_ref: "child1".to_string(),
            position: "3".to_string(),
            address: Some("5".to_string()),
        };
        let pub_cm = resolve_connected_module(&model_cm).unwrap();
        assert_eq!(pub_cm.child_id_ref, "child1");
        assert_eq!(pub_cm.position, 3);
        assert_eq!(pub_cm.address, Some(5));
    }

    #[test]
    fn test_resolve_interface_device() {
        let model_if = model_mod::InterfaceDevice {
            unique_id: "if1".to_string(),
            interface_type: "X2X".to_string(),
            max_modules: "10".to_string(),
            module_addressing: model_mod::ModuleAddressingHead::Position,
            file_list: model_mod::FileList {
                file: vec![model_mod::File {
                    uri: "file1.xdd".to_string(),
                }],
            },
            connected_module_list: Some(model_mod::ConnectedModuleList {
                connected_module: vec![model_mod::ConnectedModule {
                    child_id_ref: "child1".to_string(),
                    position: "1".to_string(),
                    ..Default::default()
                }],
            }),
            ..Default::default()
        };

        let pub_if = resolve_interface_device(&model_if).unwrap();
        assert_eq!(pub_if.unique_id, "if1");
        assert_eq!(pub_if.max_modules, 10);
        assert_eq!(pub_if.module_addressing, "position");
        assert_eq!(pub_if.file_list.len(), 1);
        assert_eq!(pub_if.file_list[0], "file1.xdd");
        assert_eq!(pub_if.connected_modules.len(), 1);
        assert_eq!(pub_if.connected_modules[0].position, 1);
    }

    #[test]
    fn test_resolve_module_interface() {
        let model_mi = model_mod::ModuleInterface {
            child_id: "child_abc".to_string(),
            interface_type: "X2X".to_string(),
            module_addressing: model_mod::ModuleAddressingChild::Next,
            ..Default::default()
        };

        let pub_mi = resolve_module_interface(&model_mi).unwrap();
        assert_eq!(pub_mi.child_id, "child_abc");
        assert_eq!(pub_mi.interface_type, "X2X");
        assert_eq!(pub_mi.module_addressing, "next");
    }

    #[test]
    fn test_resolve_module_management_device() {
        let model_mmd = model_mod::ModuleManagementDevice {
            interface_list: model_mod::InterfaceListDevice {
                interface: vec![model_mod::InterfaceDevice {
                    unique_id: "if1".to_string(),
                    interface_type: "X2X".to_string(),
                    max_modules: "10".to_string(),
                    ..Default::default()
                }],
            },
            module_interface: Some(model_mod::ModuleInterface {
                child_id: "child_abc".to_string(),
                ..Default::default()
            }),
        };

        let pub_mmd = resolve_module_management_device(&model_mmd).unwrap();
        assert_eq!(pub_mmd.interfaces.len(), 1);
        assert_eq!(pub_mmd.interfaces[0].unique_id, "if1");
        assert!(pub_mmd.module_interface.is_some());
        assert_eq!(pub_mmd.module_interface.unwrap().child_id, "child_abc");
    }

    // --- resolve_module_management_comm tests ---

    #[test]
    fn test_resolve_range() {
        let model_range = model_mod::Range {
            name: "DigitalInputs".to_string(),
            base_index: "6000".to_string(),
            max_index: Some("60FF".to_string()),
            max_sub_index: "FE".to_string(),
            sort_mode: model_mod::SortMode::Index,
            sort_number: model_mod::AddressingAttribute::Address,
            sort_step: Some("16".to_string()),
            pdo_mapping: Some(ObjectPdoMapping::Rpdo),
        };

        let pub_range = resolve_range(&model_range).unwrap();
        assert_eq!(pub_range.name, "DigitalInputs");
        assert_eq!(pub_range.base_index, 0x6000);
        assert_eq!(pub_range.max_index, Some(0x60FF));
        assert_eq!(pub_range.max_sub_index, 0xFE);
        assert_eq!(pub_range.sort_mode, "index");
        assert_eq!(pub_range.sort_number, "address");
        assert_eq!(pub_range.sort_step, Some(16));
        assert_eq!(pub_range.pdo_mapping, Some(types::ObjectPdoMapping::Rpdo));
    }

    #[test]
    fn test_resolve_range_invalid_hex() {
        let model_range = model_mod::Range {
            base_index: "NOT_HEX".to_string(),
            max_sub_index: "01".to_string(),
            ..Default::default()
        };
        assert!(matches!(
            resolve_range(&model_range),
            Err(XdcError::InvalidAttributeFormat { .. })
        ));

        let model_range_bad_step = model_mod::Range {
            base_index: "6000".to_string(),
            max_sub_index: "01".to_string(),
            sort_step: Some("NOT_A_NUM".to_string()),
            ..Default::default()
        };
        assert!(matches!(
            resolve_range(&model_range_bad_step),
            Err(XdcError::InvalidAttributeFormat {
                attribute: "range @sortStep"
            })
        ));
    }

    #[test]
    fn test_resolve_module_management_comm() {
        let model_mmc = model_mod::ModuleManagementComm {
            interface_list: model_mod::InterfaceListComm {
                interface: vec![model_mod::InterfaceComm {
                    unique_id_ref: "if1".to_string(),
                    range_list: model_mod::RangeList {
                        range: vec![model_mod::Range {
                            name: "DigitalInputs".to_string(),
                            base_index: "6000".to_string(),
                            max_sub_index: "FF".to_string(),
                            ..Default::default()
                        }],
                    },
                }],
            },
        };

        let pub_mmc = resolve_module_management_comm(&model_mmc).unwrap();
        assert_eq!(pub_mmc.interfaces.len(), 1);
        let pub_if = &pub_mmc.interfaces[0];
        assert_eq!(pub_if.unique_id_ref, "if1");
        assert_eq!(pub_if.ranges.len(), 1);
        assert_eq!(pub_if.ranges[0].name, "DigitalInputs");
        assert_eq!(pub_if.ranges[0].base_index, 0x6000);
    }
}