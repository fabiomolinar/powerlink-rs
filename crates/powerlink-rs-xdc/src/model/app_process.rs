//! Contains model structs related to `<ApplicationProcess>`.
//!
//! (Schema: `ProfileBody_Device_Powerlink.xsd`)

use super::common::{DataTypeIDRef, Glabels, bool_false, is_false};
use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

/// Represents the `<ApplicationProcess>` block.
///
/// This contains the device parameters, which serve as the source of default values.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ApplicationProcess {
    /// Contains user-defined data types (EPSG 311, 7.4.7.2).
    #[serde(
        rename = "dataTypeList",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub data_type_list: Option<AppDataTypeList>,

    /// Contains function type definitions (EPSG 311, 7.4.7.3).
    #[serde(
        rename = "functionTypeList",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub function_type_list: Option<FunctionTypeList>,

    /// Contains function instances (EPSG 311, 7.4.7.5).
    #[serde(
        rename = "functionInstanceList",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub function_instance_list: Option<FunctionInstanceList>,

    /// Contains parameter templates (EPSG 311, 7.4.7.6).
    #[serde(
        rename = "templateList",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub template_list: Option<TemplateList>,

    /// Contains parameter definitions (EPSG 311, 7.4.7.7).
    #[serde(rename = "parameterList")]
    pub parameter_list: ParameterList,

    /// Contains parameter groupings (EPSG 311, 7.4.7.8).
    #[serde(
        rename = "parameterGroupList",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub parameter_group_list: Option<ParameterGroupList>,
}

/// Represents `<templateList>` (EPSG 311, 7.4.7.6).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct TemplateList {
    #[serde(
        rename = "parameterTemplate",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub parameter_template: Vec<Parameter>, // Identical to Parameter structure

    #[serde(
        rename = "allowedValuesTemplate",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub allowed_values_template: Vec<AllowedValuesTemplate>,
}

/// Represents `<allowedValuesTemplate>` (EPSG 311, 7.4.7.6).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct AllowedValuesTemplate {
    #[serde(rename = "@uniqueID")]
    pub unique_id: String,

    #[serde(rename = "value", default, skip_serializing_if = "Vec::is_empty")]
    pub value: Vec<Value>,

    #[serde(rename = "range", default, skip_serializing_if = "Vec::is_empty")]
    pub range: Vec<Range>,
}

/// Represents `<parameterList>` (EPSG 311, 7.4.7.7).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ParameterList {
    #[serde(rename = "parameter", default, skip_serializing_if = "Vec::is_empty")]
    pub parameter: Vec<Parameter>,
}

/// Represents `access` attribute (EPSG 311, Table 40).
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum ParameterAccess {
    #[serde(rename = "const")]
    Const,
    #[serde(rename = "read")]
    Read,
    #[serde(rename = "write")]
    Write,
    #[serde(rename = "readWrite")]
    ReadWrite,
    #[serde(rename = "readWriteInput")]
    ReadWriteInput,
    #[serde(rename = "readWriteOutput")]
    ReadWriteOutput,
    #[serde(rename = "noAccess")]
    NoAccess,
}

/// Represents `support` attribute (EPSG 311, Table 40).
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum ParameterSupport {
    #[serde(rename = "mandatory")]
    Mandatory,
    #[serde(rename = "optional")]
    Optional,
    #[serde(rename = "conditional")]
    Conditional,
}

/// Represents `<variableRef>` (EPSG 311, 7.4.7.7.2.9).
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct VariableRef {
    #[serde(rename = "instanceIDRef", default)]
    pub instance_id_ref: Vec<InstanceIDRef>,

    #[serde(rename = "variableIDRef")]
    pub variable_id_ref: VariableIDRef,

    #[serde(rename = "memberRef", default, skip_serializing_if = "Vec::is_empty")]
    pub member_ref: Vec<MemberRef>,

    #[serde(rename = "@position", default, skip_serializing_if = "Option::is_none")]
    pub position: Option<u8>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct InstanceIDRef {
    #[serde(rename = "@uniqueIDRef")]
    pub unique_id_ref: String,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct VariableIDRef {
    #[serde(rename = "@uniqueIDRef")]
    pub unique_id_ref: String,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct MemberRef {
    #[serde(
        rename = "@uniqueIDRef",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub unique_id_ref: Option<String>,
    #[serde(rename = "@index", default, skip_serializing_if = "Option::is_none")]
    pub index: Option<i64>,
}

/// Represents the choice of data type for a Parameter.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ParameterDataType {
    // Simple types from GSimple (inlined)
    BOOL,
    BITSTRING,
    BYTE,
    CHAR,
    WORD,
    DWORD,
    LWORD,
    SINT,
    INT,
    DINT,
    LINT,
    USINT,
    UINT,
    UDINT,
    ULINT,
    REAL,
    LREAL,
    STRING,
    WSTRING,

    // Complex choices
    #[serde(rename = "dataTypeIDRef")]
    DataTypeIDRef(DataTypeIDRef),
    #[serde(rename = "variableRef")]
    VariableRef(VariableRef),
}

impl Default for ParameterDataType {
    fn default() -> Self {
        ParameterDataType::BOOL
    }
}

/// Represents `<conditionalSupport>` (EPSG 311, 7.4.7.7.2.2).
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct ConditionalSupport {
    #[serde(rename = "@paramIDRef")]
    pub param_id_ref: String,
}

/// Represents `<denotation>` (EPSG 311, 7.4.7.7.2.3).
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Denotation {
    #[serde(flatten)]
    pub labels: Glabels,
}

/// Represents `<allowedValues>` (EPSG 311, 7.4.7.7.2.7).
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct AllowedValues {
    #[serde(
        rename = "@templateIDRef",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub template_id_ref: Option<String>,

    #[serde(rename = "value", default, skip_serializing_if = "Vec::is_empty")]
    pub value: Vec<Value>,

    #[serde(rename = "range", default, skip_serializing_if = "Vec::is_empty")]
    pub range: Vec<Range>,
}

/// Represents `<range>` (EPSG 311, 7.4.7.7.2.7).
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Range {
    #[serde(rename = "minValue")]
    pub min_value: Value,
    #[serde(rename = "maxValue")]
    pub max_value: Value,
    #[serde(rename = "step", default, skip_serializing_if = "Option::is_none")]
    pub step: Option<Value>,
}

/// Represents `<unit>` (EPSG 311, 7.4.7.7.2.8).
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Unit {
    #[serde(flatten)]
    pub labels: Glabels,
    #[serde(
        rename = "@multiplier",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub multiplier: Option<String>,
    #[serde(rename = "@unitURI", default, skip_serializing_if = "Option::is_none")]
    pub unit_uri: Option<String>,
}

/// Represents `<property>` (EPSG 311, 7.4.7.7.2.13).
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Property {
    #[serde(rename = "@name")]
    pub name: String,
    #[serde(rename = "@value")]
    pub value: String,
}

/// Represents a `<parameter>` (EPSG 311, 7.4.7.7.2).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Parameter {
    /// The unique ID (e.g., "Param1_Vendor_Specific").
    #[serde(rename = "@uniqueID")]
    pub unique_id: String,

    #[serde(rename = "@access", default, skip_serializing_if = "Option::is_none")]
    pub access: Option<ParameterAccess>,

    #[serde(rename = "@support", default, skip_serializing_if = "Option::is_none")]
    pub support: Option<ParameterSupport>,

    #[serde(
        rename = "@persistent",
        default = "bool_false",
        skip_serializing_if = "is_false"
    )]
    pub persistent: bool,

    #[serde(rename = "@offset", default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<String>,

    #[serde(
        rename = "@multiplier",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub multiplier: Option<String>,

    /// An optional reference to a `<parameterTemplate>`.
    #[serde(
        rename = "@templateIDRef",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub template_id_ref: Option<String>,

    // --- Elements ---
    #[serde(flatten)]
    pub labels: Glabels,

    #[serde(flatten)]
    pub data_type: ParameterDataType,

    #[serde(
        rename = "conditionalSupport",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub conditional_support: Vec<ConditionalSupport>,

    #[serde(
        rename = "denotation",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub denotation: Option<Denotation>,

    /// The `actualValue` element (prioritized in XDC).
    #[serde(
        rename = "actualValue",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub actual_value: Option<Value>,

    /// The `defaultValue` element (prioritized in XDD).
    #[serde(
        rename = "defaultValue",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub default_value: Option<Value>,

    #[serde(
        rename = "substituteValue",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub substitute_value: Option<Value>,

    #[serde(
        rename = "allowedValues",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub allowed_values: Option<AllowedValues>,

    #[serde(rename = "unit", default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<Unit>,

    #[serde(rename = "property", default, skip_serializing_if = "Vec::is_empty")]
    pub property: Vec<Property>,
}

/// Represents a simple value-holding element like `<defaultValue value="0x01"/>`.
/// (EPSG 311, 7.4.7.7.2.4, 7.4.7.7.2.5).
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Value {
    #[serde(flatten, default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<Glabels>,

    #[serde(rename = "@value")]
    pub value: String,

    #[serde(rename = "@offset", default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<String>,

    #[serde(
        rename = "@multiplier",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub multiplier: Option<String>,
}

// --- Structs for ApplicationProcess dataTypeList ---

/// Represents `<dataTypeList>` in ApplicationProcess (EPSG 311, 7.4.7.2).
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct AppDataTypeList {
    #[serde(rename = "$value", default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<AppDataTypeChoice>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum AppDataTypeChoice {
    #[serde(rename = "array")]
    Array(AppArray),
    #[serde(rename = "struct")]
    Struct(AppStruct),
    #[serde(rename = "enum")]
    Enum(AppEnum),
    #[serde(rename = "derived")]
    Derived(AppDerived),
}

/// Represents `<array>` (EPSG 311, 7.4.7.2.3)
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppArray {
    #[serde(rename = "@name")]
    pub name: String,
    #[serde(rename = "@uniqueID")]
    pub unique_id: String,
    #[serde(flatten)]
    pub labels: Glabels,
    #[serde(rename = "subrange", default, skip_serializing_if = "Vec::is_empty")]
    pub subrange: Vec<Subrange>,
    #[serde(flatten)]
    pub data_type: ParameterDataType,
}

/// Represents `<subrange>` (EPSG 311, 7.4.7.2.3.2)
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Subrange {
    #[serde(rename = "@lowerLimit")]
    pub lower_limit: String, // xsd:positiveInteger
    #[serde(rename = "@upperLimit")]
    pub upper_limit: String, // xsd:positiveInteger
}

/// Represents `<struct>` (EPSG 311, 7.4.7.2.4)
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppStruct {
    #[serde(rename = "@name")]
    pub name: String,
    #[serde(rename = "@uniqueID")]
    pub unique_id: String,
    #[serde(flatten)]
    pub labels: Glabels,
    #[serde(
        rename = "varDeclaration",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub var_declaration: Vec<VarDeclaration>,
}

/// Represents `<varDeclaration>` (EPSG 311, 7.4.7.2.4.2)
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct VarDeclaration {
    #[serde(rename = "@name")]
    pub name: String,
    #[serde(rename = "@uniqueID")]
    pub unique_id: String,
    #[serde(rename = "@size", default, skip_serializing_if = "Option::is_none")]
    pub size: Option<String>, // xsd:positiveInteger
    #[serde(
        rename = "@initialValue",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub initial_value: Option<String>,
    #[serde(flatten)]
    pub labels: Glabels,
    #[serde(flatten)]
    pub data_type: ParameterDataType,
}

/// Represents `<enum>` (EPSG 311, 7.4.7.2.5)
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppEnum {
    #[serde(rename = "@name")]
    pub name: String,
    #[serde(rename = "@uniqueID")]
    pub unique_id: String,
    #[serde(rename = "@size", default, skip_serializing_if = "Option::is_none")]
    pub size: Option<String>, // xsd:positiveInteger
    #[serde(flatten)]
    pub labels: Glabels,
    #[serde(rename = "enumValue", default, skip_serializing_if = "Vec::is_empty")]
    pub enum_value: Vec<EnumValue>,
    // This choice `g_simple` is optional
    #[serde(flatten)]
    pub data_type: Option<ParameterDataType>,
}

/// Represents `<enumValue>` (EPSG 311, 7.4.7.2.5.2)
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct EnumValue {
    #[serde(rename = "@value", default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(flatten)]
    pub labels: Glabels,
}

/// Represents `<derived>` (EPSG 311, 7.4.7.2.6)
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppDerived {
    #[serde(rename = "@name")]
    pub name: String,
    #[serde(rename = "@uniqueID")]
    pub unique_id: String,
    #[serde(
        rename = "@description",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub description: Option<String>,
    #[serde(flatten)]
    pub labels: Glabels,
    #[serde(rename = "count", default, skip_serializing_if = "Option::is_none")]
    pub count: Option<Count>,
    #[serde(flatten)]
    pub data_type: ParameterDataType,
}

/// Represents `<count>` (EPSG 311, 7.4.7.2.6.2)
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Count {
    #[serde(rename = "@uniqueID")]
    pub unique_id: String,
    #[serde(rename = "@access", default, skip_serializing_if = "Option::is_none")]
    pub access: Option<ParameterAccess>,
    #[serde(flatten)]
    pub labels: Glabels,
    #[serde(rename = "defaultValue")]
    pub default_value: Value,
    #[serde(
        rename = "allowedValues",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub allowed_values: Option<AllowedValues>,
}

// --- Structs for Function Types and Instances ---

/// Represents `<functionTypeList>` (EPSG 311, 7.4.7.3)
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct FunctionTypeList {
    #[serde(
        rename = "functionType",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub function_type: Vec<FunctionType>,
}

/// Represents `<functionType>` (EPSG 311, 7.4.7.4)
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FunctionType {
    #[serde(rename = "@name")]
    pub name: String,
    #[serde(rename = "@uniqueID")]
    pub unique_id: String,
    #[serde(rename = "@package", default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    #[serde(flatten)]
    pub labels: Glabels,
    #[serde(rename = "versionInfo", default, skip_serializing_if = "Vec::is_empty")]
    pub version_info: Vec<VersionInfo>,
    #[serde(rename = "interfaceList")]
    pub interface_list: InterfaceList,
    #[serde(
        rename = "functionInstanceList",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub function_instance_list: Option<FunctionInstanceList>,
}

/// Represents `<versionInfo>` (EPSG 311, 7.4.7.4.2)
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VersionInfo {
    #[serde(rename = "@organization")]
    pub organization: String,
    #[serde(rename = "@version")]
    pub version: String,
    #[serde(rename = "@author")]
    pub author: String,
    #[serde(rename = "@date")]
    pub date: String, // xsd:date
    #[serde(flatten)]
    pub labels: Glabels,
}

/// Represents `<interfaceList>` (EPSG 311, 7.4.7.4.3)
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct InterfaceList {
    #[serde(rename = "inputVars", default, skip_serializing_if = "Option::is_none")]
    pub input_vars: Option<VarList>,
    #[serde(
        rename = "outputVars",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub output_vars: Option<VarList>,
    #[serde(
        rename = "configVars",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub config_vars: Option<VarList>,
}

/// Represents a list of `<varDeclaration>`
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct VarList {
    #[serde(
        rename = "varDeclaration",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub var_declaration: Vec<VarDeclaration>,
}

/// Represents `<functionInstanceList>` (EPSG 311, 7.4.7.5)
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct FunctionInstanceList {
    #[serde(
        rename = "functionInstance",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub function_instance: Vec<FunctionInstance>,
    #[serde(rename = "connection", default, skip_serializing_if = "Vec::is_empty")]
    pub connection: Vec<Connection>,
}

/// Represents `<functionInstance>` (EPSG 311, 7.4.7.5.2)
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FunctionInstance {
    #[serde(rename = "@name")]
    pub name: String,
    #[serde(rename = "@uniqueID")]
    pub unique_id: String,
    #[serde(rename = "@typeIDRef")]
    pub type_id_ref: String, // xsd:IDREF
    #[serde(flatten)]
    pub labels: Glabels,
}

/// Represents `<connection>` (EPSG 311, 7.4.7.5.3)
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Connection {
    #[serde(rename = "@source")]
    pub source: String,
    #[serde(rename = "@destination")]
    pub destination: String,
    #[serde(
        rename = "@description",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub description: Option<String>,
}

// --- Structs for Parameter Group List ---

/// Represents `<parameterGroupList>` (EPSG 311, 7.4.7.8)
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct ParameterGroupList {
    #[serde(
        rename = "parameterGroup",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub parameter_group: Vec<ParameterGroup>,
}

/// Represents `<parameterGroup>` (EPSG 311, 7.4.7.8.2)
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ParameterGroup {
    #[serde(rename = "@uniqueID")]
    pub unique_id: String,
    #[serde(
        rename = "@kindOfAccess",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub kind_of_access: Option<String>,
    #[serde(
        rename = "@configParameter",
        default = "bool_false",
        skip_serializing_if = "is_false"
    )]
    pub config_parameter: bool,
    #[serde(
        rename = "@groupLevelVisible",
        default = "bool_false",
        skip_serializing_if = "is_false"
    )]
    pub group_level_visible: bool,
    #[serde(
        rename = "@conditionalUniqueIDRef",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub conditional_unique_id_ref: Option<String>, // xsd:IDREF
    #[serde(
        rename = "@conditionalValue",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub conditional_value: Option<String>,
    #[serde(
        rename = "@bitOffset",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub bit_offset: Option<String>, // xsd:nonNegativeInteger

    #[serde(flatten)]
    pub labels: Glabels,

    // This choice allows nesting groups or referencing parameters
    #[serde(rename = "$value", default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<ParameterGroupItem>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ParameterGroupItem {
    #[serde(rename = "parameterGroup")]
    ParameterGroup(ParameterGroup),
    #[serde(rename = "parameterRef")]
    ParameterRef(ParameterRef),
}

/// Represents `<parameterRef>` (EPSG 311, 7.4.7.8.3)
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ParameterRef {
    #[serde(rename = "@uniqueIDRef")]
    pub unique_id_ref: String, // xsd:IDREF
    #[serde(
        rename = "@actualValue",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub actual_value: Option<String>,
    #[serde(
        rename = "@visible",
        default = "bool_false",
        skip_serializing_if = "is_false"
    )]
    pub visible: bool,
    #[serde(
        rename = "@locked",
        default = "bool_false",
        skip_serializing_if = "is_false"
    )]
    pub locked: bool,
    #[serde(
        rename = "@bitOffset",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub bit_offset: Option<String>, // xsd:nonNegativeInteger
}
