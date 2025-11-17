// crates/powerlink-rs-xdc/src/resolver/app_process.rs

//! Handles resolving the `ApplicationProcess` block from the model to public types.

use crate::error::XdcError;
use crate::model;
use crate::model::app_process::{AppDataTypeChoice, ParameterGroupItem};
// Removed unused import: use crate::model::common::{Glabels, LabelChoice};
// Removed unused import: use crate::parser::parse_hex_u8;
use crate::resolver::utils; // Import the utils module
use crate::types;
use alloc::string::String;
use alloc::vec::Vec;
// Removed unused import: use crate::types::ParameterRef;

// --- Label Helpers ---

// REMOVED: `extract_label` - Now in `utils.rs`
// REMOVED: `extract_description` - Now in `utils.rs`

/// Helper to extract the name of a `ParameterDataType` enum.
fn get_data_type_name(data_type: &model::app_process::ParameterDataType) -> String {
    use model::app_process::ParameterDataType::*;
    match data_type {
        BOOL => "BOOL".into(),
        BITSTRING => "BITSTRING".into(),
        BYTE => "BYTE".into(),
        CHAR => "CHAR".into(),
        WORD => "WORD".into(),
        DWORD => "DWORD".into(),
        LWORD => "LWORD".into(),
        SINT => "SINT".into(),
        INT => "INT".into(),
        DINT => "DINT".into(),
        LINT => "LINT".into(),
        USINT => "USINT".into(),
        UINT => "UINT".into(),
        UDINT => "UDINT".into(),
        ULINT => "ULINT".into(),
        REAL => "REAL".into(),
        LREAL => "LREAL".into(),
        STRING => "STRING".into(),
        WSTRING => "WSTRING".into(),
        DataTypeIDRef(r) => r.unique_id_ref.clone(),
        VariableRef(_) => "variableRef".into(), // This case is complex, placeholder
    }
}

// --- Main Resolver ---

/// Resolves the `model::ApplicationProcess` into the public `types::ApplicationProcess`.
pub(super) fn resolve_application_process(
    model: &model::app_process::ApplicationProcess,
) -> Result<types::ApplicationProcess, XdcError> {
    let data_types = model
        .data_type_list
        .as_ref()
        .map_or(Ok(Vec::new()), resolve_data_type_list)?;

    let parameter_groups = model
        .parameter_group_list
        .as_ref()
        .map_or(Ok(Vec::new()), resolve_parameter_group_list)?;

    let function_types = model
        .function_type_list
        .as_ref()
        .map_or(Ok(Vec::new()), resolve_function_type_list)?;

    let function_instances = model
        .function_instance_list
        .as_ref()
        .map_or(Ok(Vec::new()), resolve_function_instance_list)?;

    Ok(types::ApplicationProcess {
        data_types,
        parameter_groups,
        function_types,
        function_instances,
    })
}

/// Helper to resolve the `<dataTypeList>`.
fn resolve_data_type_list(
    list: &model::app_process::AppDataTypeList,
) -> Result<Vec<types::AppDataType>, XdcError> {
    list.items
        .iter()
        .map(|item| match item {
            AppDataTypeChoice::Struct(s) => Ok(types::AppDataType::Struct(resolve_struct(s)?)),
            AppDataTypeChoice::Array(a) => Ok(types::AppDataType::Array(resolve_array(a)?)),
            AppDataTypeChoice::Enum(e) => Ok(types::AppDataType::Enum(resolve_enum(e)?)),
            AppDataTypeChoice::Derived(d) => Ok(types::AppDataType::Derived(resolve_derived(d)?)),
        })
        .collect()
}

/// Helper to resolve a `<struct>`.
fn resolve_struct(model: &model::app_process::AppStruct) -> Result<types::AppStruct, XdcError> {
    let members = model
        .var_declaration
        .iter()
        .map(|var| {
            // This is the fix: construct a types::StructMember directly
            Ok(types::StructMember {
                name: var.name.clone(),
                unique_id: var.unique_id.clone(),
                data_type: get_data_type_name(&var.data_type),
                size: var
                    .size
                    .as_ref()
                    .and_then(|s| s.parse::<u32>().ok()),
                label: utils::extract_label(&var.labels.items), // FIX: Pass .items
                description: utils::extract_description(&var.labels.items), // FIX: Pass .items
            })
        })
        .collect::<Result<Vec<_>, XdcError>>()?;

    Ok(types::AppStruct {
        name: model.name.clone(),
        unique_id: model.unique_id.clone(),
        label: utils::extract_label(&model.labels.items), // FIX: Pass .items
        description: utils::extract_description(&model.labels.items), // FIX: Pass .items
        members,
    })
}

/// Helper to resolve an `<array>`.
fn resolve_array(model: &model::app_process::AppArray) -> Result<types::AppArray, XdcError> {
    // XDC spec (7.4.7.2.3) implies single dimension for OD-style arrays,
    // but schema allows multiple. We'll take the first <subrange>.
    let subrange = model.subrange.first().ok_or(XdcError::MissingElement {
        element: "subrange",
    })?;

    Ok(types::AppArray {
        name: model.name.clone(),
        unique_id: model.unique_id.clone(),
        label: utils::extract_label(&model.labels.items), // FIX: Pass .items
        description: utils::extract_description(&model.labels.items), // FIX: Pass .items
        lower_limit: subrange.lower_limit.parse().unwrap_or(0),
        upper_limit: subrange.upper_limit.parse().unwrap_or(0),
        data_type: get_data_type_name(&model.data_type),
    })
}

/// Helper to resolve an `<enum>`.
fn resolve_enum(model: &model::app_process::AppEnum) -> Result<types::AppEnum, XdcError> {
    let values = model
        .enum_value
        .iter()
        .map(|val| types::EnumValue {
            name: utils::extract_label(&val.labels.items).unwrap_or_default(), // Name comes from <label>
            value: val.value.clone().unwrap_or_default(),
            label: utils::extract_label(&val.labels.items), // FIX: Pass .items
            description: utils::extract_description(&val.labels.items), // FIX: Pass .items
        })
        .collect();

    Ok(types::AppEnum {
        name: model.name.clone(),
        unique_id: model.unique_id.clone(),
        label: utils::extract_label(&model.labels.items), // FIX: Pass .items
        description: utils::extract_description(&model.labels.items), // FIX: Pass .items
        data_type: model
            .data_type
            .as_ref()
            .map(get_data_type_name)
            .unwrap_or_default(),
        size_in_bits: model
            .size
            .as_ref()
            .and_then(|s| s.parse::<u32>().ok()),
        values,
    })
}

/// Helper to resolve a `<derived>` type.
fn resolve_derived(model: &model::app_process::AppDerived) -> Result<types::AppDerived, XdcError> {
    let count = model.count.as_ref().map(|c| types::Count {
        unique_id: c.unique_id.clone(),
        access: c.access.map(super::utils::map_param_access),
        default_value: Some(c.default_value.value.clone()),
    });

    Ok(types::AppDerived {
        name: model.name.clone(),
        unique_id: model.unique_id.clone(),
        label: utils::extract_label(&model.labels.items), // FIX: Pass .items
        description: utils::extract_description(&model.labels.items), // FIX: Pass .items
        data_type: get_data_type_name(&model.data_type),
        count,
    })
}

/// Helper to resolve the `<parameterGroupList>`.
fn resolve_parameter_group_list(
    list: &model::app_process::ParameterGroupList,
) -> Result<Vec<types::ParameterGroup>, XdcError> {
    list.parameter_group
        .iter()
        .map(resolve_parameter_group)
        .collect()
}

/// Helper to recursively resolve a `<parameterGroup>`.
fn resolve_parameter_group(
    model: &model::app_process::ParameterGroup,
) -> Result<types::ParameterGroup, XdcError> {
    let items = model
        .items
        .iter()
        .map(|item| match item {
            ParameterGroupItem::ParameterGroup(pg) => {
                Ok(types::ParameterGroupItem::Group(resolve_parameter_group(pg)?))
            }
            ParameterGroupItem::ParameterRef(pr) => {
                Ok(types::ParameterGroupItem::Parameter(types::ParameterRef {
                    unique_id_ref: pr.unique_id_ref.clone(),
                    visible: pr.visible,
                    locked: pr.locked,
                    bit_offset: pr
                        .bit_offset
                        .as_ref()
                        .and_then(|s| s.parse::<u32>().ok()),
                }))
            }
        })
        .collect::<Result<Vec<_>, XdcError>>()?;

    Ok(types::ParameterGroup {
        unique_id: model.unique_id.clone(),
        label: utils::extract_label(&model.labels.items), // FIX: Pass .items
        description: utils::extract_description(&model.labels.items), // FIX: Pass .items
        items,
    })
}

/// Helper to resolve `<functionTypeList>`.
fn resolve_function_type_list(
    list: &model::app_process::FunctionTypeList,
) -> Result<Vec<types::FunctionType>, XdcError> {
    list.function_type
        .iter()
        .map(resolve_function_type)
        .collect()
}

/// Helper to resolve a single `<functionType>`.
fn resolve_function_type(
    model: &model::app_process::FunctionType,
) -> Result<types::FunctionType, XdcError> {
    let version_info = model
        .version_info
        .iter()
        .map(|v| types::VersionInfo {
            organization: v.organization.clone(),
            version: v.version.clone(),
            author: v.author.clone(),
            date: v.date.clone(),
            label: utils::extract_label(&v.labels.items), // FIX: Pass .items
            description: utils::extract_description(&v.labels.items), // FIX: Pass .items
        })
        .collect();

    let interface = resolve_interface_list(&model.interface_list)?;

    // Note: We are not resolving the nested `functionInstanceList` inside a
    // `functionType` yet, as it's less critical for OD mapping.

    Ok(types::FunctionType {
        name: model.name.clone(),
        unique_id: model.unique_id.clone(),
        package: model.package.clone(),
        label: utils::extract_label(&model.labels.items), // FIX: Pass .items
        description: utils::extract_description(&model.labels.items), // FIX: Pass .items
        version_info,
        interface,
    })
}

/// Helper to resolve an `<interfaceList>`.
fn resolve_interface_list(
    model: &model::app_process::InterfaceList,
) -> Result<types::InterfaceList, XdcError> {
    let inputs = model
        .input_vars
        .as_ref()
        .map_or(Ok(Vec::new()), |vars| {
            vars.var_declaration
                .iter()
                .map(resolve_var_declaration)
                .collect()
        })?;

    let outputs = model
        .output_vars
        .as_ref()
        .map_or(Ok(Vec::new()), |vars| {
            vars.var_declaration
                .iter()
                .map(resolve_var_declaration)
                .collect()
        })?;

    let configs = model
        .config_vars
        .as_ref()
        .map_or(Ok(Vec::new()), |vars| {
            vars.var_declaration
                .iter()
                .map(resolve_var_declaration)
                .collect()
        })?;

    Ok(types::InterfaceList {
        inputs,
        outputs,
        configs,
    })
}

/// Helper to resolve a `<varDeclaration>`.
fn resolve_var_declaration(
    model: &model::app_process::VarDeclaration,
) -> Result<types::VarDeclaration, XdcError> {
    Ok(types::VarDeclaration {
        name: model.name.clone(),
        unique_id: model.unique_id.clone(),
        data_type: get_data_type_name(&model.data_type),
        size: model.size.as_ref().and_then(|s| s.parse::<u32>().ok()),
        initial_value: model.initial_value.clone(),
        label: utils::extract_label(&model.labels.items), // FIX: Pass .items
        description: utils::extract_description(&model.labels.items), // FIX: Pass .items
    })
}

/// Helper to resolve `<functionInstanceList>`.
fn resolve_function_instance_list(
    list: &model::app_process::FunctionInstanceList,
) -> Result<Vec<types::FunctionInstance>, XdcError> {
    list.function_instance
        .iter()
        .map(|inst| {
            Ok(types::FunctionInstance {
                name: inst.name.clone(),
                unique_id: inst.unique_id.clone(),
                type_id_ref: inst.type_id_ref.clone(),
                label: utils::extract_label(&inst.labels.items), // FIX: Pass .items
                description: utils::extract_description(&inst.labels.items), // FIX: Pass .items
            })
        })
        .collect()
    // Note: We are not resolving <connection> elements yet.
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::app_process::{
        AppArray, AppDerived, AppEnum, AppStruct, Count,
        EnumValue, FunctionInstance, FunctionInstanceList, FunctionType, FunctionTypeList,
        InterfaceList, ParameterDataType, ParameterGroup, ParameterGroupItem, ParameterRef,
        Subrange, VarDeclaration, VarList, VersionInfo,
    };
    use crate::model::common::{DataTypeIDRef, Glabels, Label, LabelChoice};
    use crate::resolver::utils::{extract_description, extract_label};
    use crate::types;
    use alloc::string::ToString;
    use alloc::vec;

    // --- Helper Function Tests ---

    #[test]
    fn test_extract_label() {
        let items = vec![
            LabelChoice::Description(Default::default()),
            LabelChoice::Label(Label {
                lang: "en".into(),
                value: "Test Label".into(),
            }),
        ];
        assert_eq!(extract_label(&items), Some("Test Label".to_string()));

        let items_no_label = vec![LabelChoice::Description(Default::default())];
        assert_eq!(extract_label(&items_no_label), None);
    }

    #[test]
    fn test_extract_description() {
        let items = vec![
            LabelChoice::Label(Default::default()),
            LabelChoice::Description(model::common::Description {
                lang: "en".into(),
                value: "Test Desc".into(),
                ..Default::default()
            }),
        ];
        assert_eq!(
            extract_description(&items),
            Some("Test Desc".to_string())
        );

        let items_no_desc = vec![LabelChoice::Label(Default::default())];
        assert_eq!(extract_description(&items_no_desc), None);
    }

    #[test]
    fn test_get_data_type_name() {
        assert_eq!(
            get_data_type_name(&ParameterDataType::UINT),
            "UINT".to_string()
        );
        assert_eq!(
            get_data_type_name(&ParameterDataType::BITSTRING),
            "BITSTRING".to_string()
        );
        let dt_ref = ParameterDataType::DataTypeIDRef(DataTypeIDRef {
            unique_id_ref: "MyStructType".to_string(),
        });
        assert_eq!(get_data_type_name(&dt_ref), "MyStructType".to_string());
    }

    // --- Main Resolver Function Tests ---

    #[test]
    fn test_resolve_var_declaration() {
        let model_var = VarDeclaration {
            name: "TestVar".to_string(),
            unique_id: "uid_var_1".to_string(),
            size: Some("16".to_string()),
            initial_value: Some("123".to_string()),
            labels: Glabels {
                items: vec![LabelChoice::Label(Label {
                    lang: "en".into(),
                    value: "My Variable".into(),
                })],
            },
            data_type: ParameterDataType::UINT,
        };

        let pub_var = resolve_var_declaration(&model_var).unwrap();
        assert_eq!(pub_var.name, "TestVar");
        assert_eq!(pub_var.unique_id, "uid_var_1");
        assert_eq!(pub_var.data_type, "UINT");
        assert_eq!(pub_var.size, Some(16));
        assert_eq!(pub_var.initial_value, Some("123".to_string()));
        assert_eq!(pub_var.label, Some("My Variable".to_string()));
    }

    #[test]
    fn test_resolve_struct() {
        let model_struct = AppStruct {
            name: "MyStruct".to_string(),
            unique_id: "uid_struct_1".to_string(),
            labels: Glabels {
                items: vec![LabelChoice::Label(Label {
                    lang: "en".into(),
                    value: "A Struct".into(),
                })],
            },
            var_declaration: vec![VarDeclaration {
                name: "Member1".to_string(),
                unique_id: "uid_member_1".to_string(),
                data_type: ParameterDataType::BOOL,
                ..Default::default()
            }],
        };

        let pub_struct = resolve_struct(&model_struct).unwrap();
        assert_eq!(pub_struct.name, "MyStruct");
        assert_eq!(pub_struct.unique_id, "uid_struct_1");
        assert_eq!(pub_struct.label, Some("A Struct".to_string()));
        assert_eq!(pub_struct.members.len(), 1);
        assert_eq!(pub_struct.members[0].name, "Member1");
        assert_eq!(pub_struct.members[0].data_type, "BOOL");
    }

    #[test]
    fn test_resolve_array() {
        let model_array = AppArray {
            name: "MyArray".to_string(),
            unique_id: "uid_array_1".to_string(),
            labels: Default::default(),
            subrange: vec![Subrange {
                lower_limit: "1".to_string(),
                upper_limit: "10".to_string(),
            }],
            data_type: ParameterDataType::DINT,
        };

        let pub_array = resolve_array(&model_array).unwrap();
        assert_eq!(pub_array.name, "MyArray");
        assert_eq!(pub_array.unique_id, "uid_array_1");
        assert_eq!(pub_array.lower_limit, 1);
        assert_eq!(pub_array.upper_limit, 10);
        assert_eq!(pub_array.data_type, "DINT");
    }

    #[test]
    fn test_resolve_enum() {
        let model_enum = AppEnum {
            name: "MyEnum".to_string(),
            unique_id: "uid_enum_1".to_string(),
            size: Some("8".to_string()),
            labels: Default::default(),
            enum_value: vec![EnumValue {
                value: Some("0".to_string()),
                labels: Glabels {
                    items: vec![LabelChoice::Label(Label {
                        lang: "en".into(),
                        value: "Off".into(),
                    })],
                },
            }],
            data_type: Some(ParameterDataType::USINT),
        };

        let pub_enum = resolve_enum(&model_enum).unwrap();
        assert_eq!(pub_enum.name, "MyEnum");
        assert_eq!(pub_enum.unique_id, "uid_enum_1");
        assert_eq!(pub_enum.data_type, "USINT");
        assert_eq!(pub_enum.size_in_bits, Some(8));
        assert_eq!(pub_enum.values.len(), 1);
        assert_eq!(pub_enum.values[0].name, "Off");
        assert_eq!(pub_enum.values[0].value, "0");
    }

    #[test]
    fn test_resolve_derived() {
        let model_derived = AppDerived {
            name: "MyDerived".to_string(),
            unique_id: "uid_derived_1".to_string(),
            description: None,
            labels: Default::default(),
            count: Some(Count {
                unique_id: "uid_count_1".to_string(),
                access: Some(model::app_process::ParameterAccess::Const),
                default_value: model::app_process::Value {
                    value: "16".to_string(),
                    ..Default::default()
                },
                allowed_values: None,
                labels: Default::default(),
            }),
            data_type: ParameterDataType::BITSTRING,
        };

        let pub_derived = resolve_derived(&model_derived).unwrap();
        assert_eq!(pub_derived.name, "MyDerived");
        assert_eq!(pub_derived.unique_id, "uid_derived_1");
        assert_eq!(pub_derived.data_type, "BITSTRING");
        assert!(pub_derived.count.is_some());
        let count = pub_derived.count.unwrap();
        assert_eq!(count.unique_id, "uid_count_1");
        assert_eq!(count.access, Some(types::ParameterAccess::Constant));
        assert_eq!(count.default_value, Some("16".to_string()));
    }

    #[test]
    fn test_resolve_parameter_group() {
        let model_group = ParameterGroup {
            unique_id: "uid_group_1".to_string(),
            labels: Glabels {
                items: vec![LabelChoice::Label(Label {
                    lang: "en".into(),
                    value: "My Group".into(),
                })],
            },
            items: vec![
                ParameterGroupItem::ParameterRef(ParameterRef {
                    unique_id_ref: "param_ref_1".to_string(),
                    visible: true,
                    locked: false,
                    bit_offset: Some("8".to_string()),
                    ..Default::default()
                }),
                ParameterGroupItem::ParameterGroup(ParameterGroup {
                    unique_id: "uid_group_nested".to_string(),
                    labels: Default::default(),
                    items: vec![ParameterGroupItem::ParameterRef(ParameterRef {
                        unique_id_ref: "param_ref_2".to_string(),
                        ..Default::default()
                    })],
                    ..Default::default()
                }),
            ],
            ..Default::default()
        };

        let pub_group = resolve_parameter_group(&model_group).unwrap();
        assert_eq!(pub_group.unique_id, "uid_group_1");
        assert_eq!(pub_group.label, Some("My Group".to_string()));
        assert_eq!(pub_group.items.len(), 2);

        // Check ParameterRef
        if let types::ParameterGroupItem::Parameter(param) = &pub_group.items[0] {
            assert_eq!(param.unique_id_ref, "param_ref_1");
            assert_eq!(param.visible, true);
            assert_eq!(param.locked, false);
            assert_eq!(param.bit_offset, Some(8));
        } else {
            panic!("Expected ParameterRef");
        }

        // Check nested Group
        if let types::ParameterGroupItem::Group(group) = &pub_group.items[1] {
            assert_eq!(group.unique_id, "uid_group_nested");
            assert_eq!(group.items.len(), 1);
            if let types::ParameterGroupItem::Parameter(param) = &group.items[0] {
                assert_eq!(param.unique_id_ref, "param_ref_2");
            } else {
                panic!("Expected nested ParameterRef");
            }
        } else {
            panic!("Expected nested ParameterGroup");
        }
    }

    #[test]
    fn test_resolve_function_type_and_instance() {
        let model_app_proc = model::app_process::ApplicationProcess {
            function_type_list: Some(FunctionTypeList {
                function_type: vec![FunctionType {
                    name: "MyFunction".to_string(),
                    unique_id: "uid_func_type_1".to_string(),
                    package: None,
                    labels: Default::default(),
                    version_info: vec![VersionInfo {
                        organization: "EPSG".to_string(),
                        version: "1.0".to_string(),
                        author: "Test".to_string(),
                        date: "2024-01-01".to_string(),
                        labels: Default::default(),
                    }],
                    interface_list: InterfaceList {
                        input_vars: Some(VarList {
                            var_declaration: vec![VarDeclaration {
                                name: "InVar".to_string(),
                                unique_id: "uid_invar_1".to_string(),
                                data_type: ParameterDataType::BOOL,
                                ..Default::default()
                            }],
                        }),
                        ..Default::default()
                    },
                    function_instance_list: None,
                }],
            }),
            function_instance_list: Some(FunctionInstanceList {
                function_instance: vec![FunctionInstance {
                    name: "Instance1".to_string(),
                    unique_id: "uid_instance_1".to_string(),
                    type_id_ref: "uid_func_type_1".to_string(),
                    labels: Default::default(),
                }],
                connection: vec![],
            }),
            ..Default::default()
        };

        let pub_app_proc = resolve_application_process(&model_app_proc).unwrap();

        // Check FunctionType
        assert_eq!(pub_app_proc.function_types.len(), 1);
        let ft = &pub_app_proc.function_types[0];
        assert_eq!(ft.name, "MyFunction");
        assert_eq!(ft.unique_id, "uid_func_type_1");
        assert_eq!(ft.version_info.len(), 1);
        assert_eq!(ft.version_info[0].version, "1.0");
        assert_eq!(ft.interface.inputs.len(), 1);
        assert_eq!(ft.interface.inputs[0].name, "InVar");

        // Check FunctionInstance
        assert_eq!(pub_app_proc.function_instances.len(), 1);
        let fi = &pub_app_proc.function_instances[0];
        assert_eq!(fi.name, "Instance1");
        assert_eq!(fi.unique_id, "uid_instance_1");
        assert_eq!(fi.type_id_ref, "uid_func_type_1");
    }
}