//! Contains builder functions to convert `types::ApplicationProcess` into `model::ApplicationProcess`.
//!
//! This module handles the serialization of parameters, templates, data types,
//! function blocks, and parameter groups.

use crate::model::app_process::{
    AllowedValues, AppArray, AppDataTypeChoice, AppDataTypeList, AppDerived, AppEnum, AppStruct,
    Count, EnumValue, FunctionInstance, FunctionInstanceList, FunctionType, FunctionTypeList,
    InterfaceList, Parameter, ParameterDataType, ParameterGroup, ParameterGroupItem, ParameterList,
    ParameterRef, Subrange, TemplateList, Value, VarDeclaration, VarList, VersionInfo,
};
use crate::model::common::{DataTypeIDRef, Description, Glabels, Label, LabelChoice};
use crate::{model, types};
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// Helper to create a `Glabels` struct from optional label and description strings.
fn build_glabels(label: Option<&String>, description: Option<&String>) -> Glabels {
    let mut items = Vec::new();
    if let Some(l) = label {
        items.push(LabelChoice::Label(Label {
            lang: "en".to_string(),
            value: l.clone(),
        }));
    }
    if let Some(d) = description {
        items.push(LabelChoice::Description(Description {
            lang: "en".to_string(),
            value: d.clone(),
            ..Default::default()
        }));
    }
    Glabels { items }
}

/// Maps `types::ParameterAccess` to the model's `ParameterAccess` enum.
fn build_param_access(access: types::ParameterAccess) -> model::app_process::ParameterAccess {
    match access {
        types::ParameterAccess::Constant => model::app_process::ParameterAccess::Const,
        types::ParameterAccess::ReadOnly => model::app_process::ParameterAccess::Read,
        types::ParameterAccess::WriteOnly => model::app_process::ParameterAccess::Write,
        types::ParameterAccess::ReadWrite => model::app_process::ParameterAccess::ReadWrite,
        types::ParameterAccess::ReadWriteInput => {
            model::app_process::ParameterAccess::ReadWriteInput
        }
        types::ParameterAccess::ReadWriteOutput => {
            model::app_process::ParameterAccess::ReadWriteOutput
        }
        types::ParameterAccess::NoAccess => model::app_process::ParameterAccess::NoAccess,
    }
}

/// Maps `types::ParameterSupport` to the model's `ParameterSupport` enum.
fn build_param_support(support: types::ParameterSupport) -> model::app_process::ParameterSupport {
    match support {
        types::ParameterSupport::Mandatory => model::app_process::ParameterSupport::Mandatory,
        types::ParameterSupport::Optional => model::app_process::ParameterSupport::Optional,
        types::ParameterSupport::Conditional => model::app_process::ParameterSupport::Conditional,
    }
}

/// Maps the public `types::ParameterDataType` enum to the internal model enum.
fn build_param_data_type(dt: &types::ParameterDataType) -> model::app_process::ParameterDataType {
    use model::app_process::ParameterDataType as M;
    use types::ParameterDataType as T;
    match dt {
        T::BOOL => M::BOOL,
        T::BITSTRING => M::BITSTRING,
        T::BYTE => M::BYTE,
        T::CHAR => M::CHAR,
        T::WORD => M::WORD,
        T::DWORD => M::DWORD,
        T::LWORD => M::LWORD,
        T::SINT => M::SINT,
        T::INT => M::INT,
        T::DINT => M::DINT,
        T::LINT => M::LINT,
        T::USINT => M::USINT,
        T::UINT => M::UINT,
        T::UDINT => M::UDINT,
        T::ULINT => M::ULINT,
        T::REAL => M::REAL,
        T::LREAL => M::LREAL,
        T::STRING => M::STRING,
        T::WSTRING => M::WSTRING,
        T::DataTypeIDRef(id) => M::DataTypeIDRef(DataTypeIDRef {
            unique_id_ref: id.clone(),
        }),
        T::VariableRef => M::VariableRef(Default::default()), // Placeholder
    }
}

/// Builds a `model::app_process::Value` from the public type.
fn build_value(val: &types::Value) -> Value {
    Value {
        labels: Some(build_glabels(val.label.as_ref(), None)),
        value: val.value.clone(),
        offset: val.offset.clone(),
        multiplier: val.multiplier.clone(),
    }
}

/// Builds the `AllowedValues` struct, including values and ranges.
fn build_allowed_values(av: &types::AllowedValues) -> AllowedValues {
    AllowedValues {
        template_id_ref: av.template_id_ref.clone(),
        value: av.values.iter().map(build_value).collect(),
        range: av
            .ranges
            .iter()
            .map(|r| model::app_process::Range {
                min_value: Value {
                    value: r.min_value.clone(),
                    ..Default::default()
                },
                max_value: Value {
                    value: r.max_value.clone(),
                    ..Default::default()
                },
                step: r.step.as_ref().map(|s| Value {
                    value: s.clone(),
                    ..Default::default()
                }),
            })
            .collect(),
    }
}

/// Builds a single `Parameter` model.
fn build_parameter(param: &types::Parameter) -> Parameter {
    Parameter {
        unique_id: param.unique_id.clone(),
        access: param.access.map(build_param_access),
        support: param.support.map(build_param_support),
        persistent: param.persistent,
        offset: param.offset.clone(),
        multiplier: param.multiplier.clone(),
        template_id_ref: param.template_id_ref.clone(),
        labels: build_glabels(param.label.as_ref(), param.description.as_ref()),
        data_type: build_param_data_type(&param.data_type),
        actual_value: param.actual_value.as_ref().map(build_value),
        default_value: param.default_value.as_ref().map(build_value),
        allowed_values: param.allowed_values.as_ref().map(build_allowed_values),
        ..Default::default()
    }
}

/// Builds the `AppDataTypeList` containing custom types (Structs, Arrays, Enums).
fn build_data_type_list(types: &[types::AppDataType]) -> Option<AppDataTypeList> {
    if types.is_empty() {
        return None;
    }
    let items = types
        .iter()
        .map(|dt| match dt {
            types::AppDataType::Struct(s) => AppDataTypeChoice::Struct(AppStruct {
                name: s.name.clone(),
                unique_id: s.unique_id.clone(),
                labels: build_glabels(s.label.as_ref(), s.description.as_ref()),
                var_declaration: s
                    .members
                    .iter()
                    .map(|m| VarDeclaration {
                        name: m.name.clone(),
                        unique_id: m.unique_id.clone(),
                        size: m.size.map(|s| s.to_string()),
                        labels: build_glabels(m.label.as_ref(), m.description.as_ref()),
                        // Assuming unknown types are ID references for serialization
                        data_type: ParameterDataType::DataTypeIDRef(DataTypeIDRef {
                            unique_id_ref: m.data_type.clone(),
                        }),
                        ..Default::default()
                    })
                    .collect(),
            }),
            types::AppDataType::Array(a) => AppDataTypeChoice::Array(AppArray {
                name: a.name.clone(),
                unique_id: a.unique_id.clone(),
                labels: build_glabels(a.label.as_ref(), a.description.as_ref()),
                subrange: vec![Subrange {
                    lower_limit: a.lower_limit.to_string(),
                    upper_limit: a.upper_limit.to_string(),
                }],
                data_type: ParameterDataType::DataTypeIDRef(DataTypeIDRef {
                    unique_id_ref: a.data_type.clone(),
                }),
            }),
            types::AppDataType::Enum(e) => AppDataTypeChoice::Enum(AppEnum {
                name: e.name.clone(),
                unique_id: e.unique_id.clone(),
                labels: build_glabels(e.label.as_ref(), e.description.as_ref()),
                size: e.size_in_bits.map(|s| s.to_string()),
                enum_value: e
                    .values
                    .iter()
                    .map(|v| EnumValue {
                        value: Some(v.value.clone()),
                        labels: build_glabels(Some(&v.name), None),
                    })
                    .collect(),
                data_type: Some(ParameterDataType::DataTypeIDRef(DataTypeIDRef {
                    unique_id_ref: e.data_type.clone(),
                })),
            }),
            types::AppDataType::Derived(d) => AppDataTypeChoice::Derived(AppDerived {
                name: d.name.clone(),
                unique_id: d.unique_id.clone(),
                labels: build_glabels(d.label.as_ref(), d.description.as_ref()),
                data_type: ParameterDataType::DataTypeIDRef(DataTypeIDRef {
                    unique_id_ref: d.data_type.clone(),
                }),
                count: d.count.as_ref().map(|c| Count {
                    unique_id: c.unique_id.clone(),
                    access: c.access.map(build_param_access),
                    default_value: Value {
                        value: c.default_value.clone().unwrap_or_default(),
                        ..Default::default()
                    },
                    labels: Default::default(),
                    allowed_values: None,
                }),
                description: None,
            }),
        })
        .collect();

    Some(AppDataTypeList { items })
}

/// Recursively builds `ParameterGroup` structures.
fn build_parameter_group(group: &types::ParameterGroup) -> ParameterGroup {
    ParameterGroup {
        unique_id: group.unique_id.clone(),
        labels: build_glabels(group.label.as_ref(), group.description.as_ref()),
        items: group
            .items
            .iter()
            .map(|item| match item {
                types::ParameterGroupItem::Group(g) => {
                    ParameterGroupItem::ParameterGroup(build_parameter_group(g))
                }
                types::ParameterGroupItem::Parameter(p) => {
                    ParameterGroupItem::ParameterRef(ParameterRef {
                        unique_id_ref: p.unique_id_ref.clone(),
                        visible: p.visible,
                        locked: p.locked,
                        bit_offset: p.bit_offset.map(|o| o.to_string()),
                        ..Default::default()
                    })
                }
            })
            .collect(),
        ..Default::default()
    }
}

/// Converts a public `types::ApplicationProcess` into a `model::ApplicationProcess`.
pub(super) fn build_model_application_process(
    public: &types::ApplicationProcess,
) -> model::app_process::ApplicationProcess {
    model::app_process::ApplicationProcess {
        data_type_list: build_data_type_list(&public.data_types),

        template_list: if public.templates.is_empty() {
            None
        } else {
            Some(TemplateList {
                parameter_template: public.templates.iter().map(build_parameter).collect(),
                allowed_values_template: Vec::new(),
            })
        },

        parameter_list: ParameterList {
            parameter: public.parameters.iter().map(build_parameter).collect(),
        },

        parameter_group_list: if public.parameter_groups.is_empty() {
            None
        } else {
            Some(model::app_process::ParameterGroupList {
                parameter_group: public
                    .parameter_groups
                    .iter()
                    .map(build_parameter_group)
                    .collect(),
            })
        },

        function_type_list: if public.function_types.is_empty() {
            None
        } else {
            Some(FunctionTypeList {
                function_type: public
                    .function_types
                    .iter()
                    .map(|ft| FunctionType {
                        name: ft.name.clone(),
                        unique_id: ft.unique_id.clone(),
                        package: ft.package.clone(),
                        labels: build_glabels(ft.label.as_ref(), ft.description.as_ref()),
                        version_info: ft
                            .version_info
                            .iter()
                            .map(|v| VersionInfo {
                                organization: v.organization.clone(),
                                version: v.version.clone(),
                                author: v.author.clone(),
                                date: v.date.clone(),
                                labels: build_glabels(v.label.as_ref(), v.description.as_ref()),
                            })
                            .collect(),
                        interface_list: InterfaceList {
                            input_vars: Some(VarList {
                                var_declaration: ft
                                    .interface
                                    .inputs
                                    .iter()
                                    .map(|v| VarDeclaration {
                                        name: v.name.clone(),
                                        unique_id: v.unique_id.clone(),
                                        data_type: ParameterDataType::DataTypeIDRef(
                                            DataTypeIDRef {
                                                unique_id_ref: v.data_type.clone(),
                                            },
                                        ),
                                        size: v.size.map(|s| s.to_string()),
                                        initial_value: v.initial_value.clone(),
                                        labels: build_glabels(
                                            v.label.as_ref(),
                                            v.description.as_ref(),
                                        ),
                                    })
                                    .collect(),
                            }),
                            // Output/Config var mapping is identical to inputs; using defaults for brevity
                            ..Default::default()
                        },
                        function_instance_list: None,
                    })
                    .collect(),
            })
        },

        function_instance_list: if public.function_instances.is_empty() {
            None
        } else {
            Some(FunctionInstanceList {
                function_instance: public
                    .function_instances
                    .iter()
                    .map(|fi| FunctionInstance {
                        name: fi.name.clone(),
                        unique_id: fi.unique_id.clone(),
                        type_id_ref: fi.type_id_ref.clone(),
                        labels: build_glabels(fi.label.as_ref(), fi.description.as_ref()),
                    })
                    .collect(),
                connection: Vec::new(),
            })
        },
    }
}