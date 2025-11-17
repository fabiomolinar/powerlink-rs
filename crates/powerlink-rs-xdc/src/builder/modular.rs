// crates/powerlink-rs-xdc/src/builder/modular.rs

//! Contains builder functions to convert modular `types` into modular `model` structs.

use crate::{model, types};
use alloc::format;
use alloc::string::ToString;

// --- Device Profile Builders ---

/// Converts a public `types::InterfaceDevice` into a `model::modular::InterfaceDevice`.
fn build_model_interface_device(
    public: &types::InterfaceDevice,
) -> model::modular::InterfaceDevice {
    model::modular::InterfaceDevice {
        unique_id: public.unique_id.clone(),
        interface_type: public.interface_type.clone(),
        max_modules: public.max_modules.to_string(),
        unused_slots: false, // TODO: This field is missing from `types::InterfaceDevice`
        module_addressing: match public.module_addressing.as_str() {
            "manual" => model::modular::ModuleAddressingHead::Manual,
            "position" => model::modular::ModuleAddressingHead::Position,
            _ => model::modular::ModuleAddressingHead::Position, // Default
        },
        file_list: model::modular::FileList {
            file: public
                .file_list
                .iter()
                .map(|uri| model::modular::File { uri: uri.clone() })
                .collect(),
        },
        connected_module_list: Some(model::modular::ConnectedModuleList {
            connected_module: public
                .connected_modules
                .iter()
                .map(|m| model::modular::ConnectedModule {
                    child_id_ref: m.child_id_ref.clone(),
                    position: m.position.to_string(),
                    address: m.address.map(|a| a.to_string()),
                })
                .collect(),
        }),
        ..Default::default()
    }
}

/// Converts a public `types::ModuleInterface` into a `model::modular::ModuleInterface`.
fn build_model_module_interface(
    public: &types::ModuleInterface,
) -> model::modular::ModuleInterface {
    model::modular::ModuleInterface {
        child_id: public.child_id.clone(),
        interface_type: public.interface_type.clone(),
        module_addressing: match public.module_addressing.as_str() {
            "manual" => model::modular::ModuleAddressingChild::Manual,
            "position" => model::modular::ModuleAddressingChild::Position,
            "next" => model::modular::ModuleAddressingChild::Next,
            _ => model::modular::ModuleAddressingChild::Position, // Default
        },
        ..Default::default() // fileList, moduleTypeList, etc., are not serialized from types
    }
}

/// Converts a public `types::ModuleManagementDevice` into a `model::modular::ModuleManagementDevice`.
pub(super) fn build_model_module_management_device(
    public: &types::ModuleManagementDevice,
) -> model::modular::ModuleManagementDevice {
    model::modular::ModuleManagementDevice {
        interface_list: model::modular::InterfaceListDevice {
            interface: public
                .interfaces
                .iter()
                .map(build_model_interface_device)
                .collect(),
        },
        module_interface: public
            .module_interface
            .as_ref()
            .map(build_model_module_interface),
    }
}

// --- Communication Profile Builders ---

/// Converts a public `types::Range` into a `model::modular::Range`.
fn build_model_range(public: &types::Range) -> model::modular::Range {
    model::modular::Range {
        name: public.name.clone(),
        base_index: format!("{:04X}", public.base_index),
        max_index: public.max_index.map(|idx| format!("{:04X}", idx)),
        max_sub_index: format!("{:02X}", public.max_sub_index),
        sort_mode: match public.sort_mode.as_str() {
            "index" => model::modular::SortMode::Index,
            "subindex" => model::modular::SortMode::Subindex,
            _ => model::modular::SortMode::Index, // Default
        },
        sort_number: match public.sort_number.as_str() {
            "continuous" => model::modular::AddressingAttribute::Continuous,
            "address" => model::modular::AddressingAttribute::Address,
            _ => model::modular::AddressingAttribute::Continuous, // Default
        },
        sort_step: public.sort_step.map(|s| s.to_string()),
        pdo_mapping: public.pdo_mapping.map(super::map_pdo_mapping_to_model),
    }
}

/// Converts a public `types::ModuleManagementComm` into a `model::modular::ModuleManagementComm`.
pub(super) fn build_model_module_management_comm(
    public: &types::ModuleManagementComm,
) -> model::modular::ModuleManagementComm {
    model::modular::ModuleManagementComm {
        interface_list: model::modular::InterfaceListComm {
            interface: public
                .interfaces
                .iter()
                .map(|i| model::modular::InterfaceComm {
                    unique_id_ref: i.unique_id_ref.clone(),
                    range_list: model::modular::RangeList {
                        range: i.ranges.iter().map(build_model_range).collect(),
                    },
                })
                .collect(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types;
    use alloc::vec;

    #[test]
    fn test_build_model_module_management_device() {
        // 1. Create public types
        let public_mgmt = types::ModuleManagementDevice {
            interfaces: vec![types::InterfaceDevice {
                unique_id: "if_x2x_1".to_string(),
                interface_type: "X2X".to_string(),
                max_modules: 10,
                module_addressing: "position".to_string(),
                file_list: vec!["X2X_Modules.xdd".to_string()],
                connected_modules: vec![types::ConnectedModule {
                    child_id_ref: "child_id_abc".to_string(),
                    position: 1,
                    address: Some(2),
                }],
            }],
            module_interface: Some(types::ModuleInterface {
                child_id: "this_device_as_child_id".to_string(),
                interface_type: "X2X".to_string(),
                module_addressing: "manual".to_string(),
            }),
        };

        // 2. Call the builder
        let model_mgmt = build_model_module_management_device(&public_mgmt);

        // 3. Verify InterfaceDevice
        assert_eq!(model_mgmt.interface_list.interface.len(), 1);
        let model_if = &model_mgmt.interface_list.interface[0];
        assert_eq!(model_if.unique_id, "if_x2x_1");
        assert_eq!(model_if.max_modules, "10");
        assert_eq!(
            model_if.module_addressing,
            model::modular::ModuleAddressingHead::Position
        );
        assert_eq!(model_if.file_list.file.len(), 1);
        assert_eq!(model_if.file_list.file[0].uri, "X2X_Modules.xdd");

        // 4. Verify ConnectedModule
        let conn_list = model_if.connected_module_list.as_ref().unwrap();
        assert_eq!(conn_list.connected_module.len(), 1);
        let model_conn = &conn_list.connected_module[0];
        assert_eq!(model_conn.child_id_ref, "child_id_abc");
        assert_eq!(model_conn.position, "1");
        assert_eq!(model_conn.address, Some("2".to_string()));

        // 5. Verify ModuleInterface
        let model_mi = model_mgmt.module_interface.unwrap();
        assert_eq!(model_mi.child_id, "this_device_as_child_id");
        assert_eq!(
            model_mi.module_addressing,
            model::modular::ModuleAddressingChild::Manual
        );
    }

    #[test]
    fn test_build_model_module_management_comm() {
        // 1. Create public types
        let public_mgmt = types::ModuleManagementComm {
            interfaces: vec![types::InterfaceComm {
                unique_id_ref: "if_x2x_1".to_string(),
                ranges: vec![types::Range {
                    name: "DigitalInputs".to_string(),
                    base_index: 0x3000,
                    max_index: Some(0x30FF),
                    max_sub_index: 0x01,
                    sort_mode: "index".to_string(),
                    sort_number: "continuous".to_string(),
                    sort_step: Some(1),
                    pdo_mapping: Some(types::ObjectPdoMapping::Rpdo),
                }],
            }],
        };

        // 2. Call the builder
        let model_mgmt = build_model_module_management_comm(&public_mgmt);

        // 3. Verify InterfaceComm
        assert_eq!(model_mgmt.interface_list.interface.len(), 1);
        let model_if = &model_mgmt.interface_list.interface[0];
        assert_eq!(model_if.unique_id_ref, "if_x2x_1");

        // 4. Verify Range
        assert_eq!(model_if.range_list.range.len(), 1);
        let model_range = &model_if.range_list.range[0];
        assert_eq!(model_range.name, "DigitalInputs");
        assert_eq!(model_range.base_index, "3000");
        assert_eq!(model_range.max_index, Some("30FF".to_string()));
        assert_eq!(model_range.max_sub_index, "01");
        assert_eq!(model_range.sort_mode, model::modular::SortMode::Index);
        assert_eq!(
            model_range.sort_number,
            model::modular::AddressingAttribute::Continuous
        );
        assert_eq!(model_range.sort_step, Some("1".to_string()));
        assert_eq!(
            model_range.pdo_mapping,
            Some(model::app_layers::ObjectPdoMapping::Rpdo)
        );
    }
}
