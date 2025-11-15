// crates/powerlink-rs-xdc/src/model/app_process.rs

//! Contains model structs related to `<ApplicationProcess>`.
//! (Schema: `ProfileBody_Device_Powerlink.xsd`)

use serde::{Deserialize, Serialize};
use alloc::vec::Vec;
use alloc::string::String;
// Fix: Removed unused GSimple import
use super::common::{is_false, DataTypeIDRef, Glabels};

// --- STRUCTS for ApplicationProcess (Task 5) ---

/// Represents the `<ApplicationProcess>` block (EPSG 311, 7.4.7).
/// This contains the device parameters, which are the source of default values.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ApplicationProcess {
    /// Contains parameter definitions (EPSG 311, 7.4.7.7).
    #[serde(rename = "parameterList", default, skip_serializing_if = "Option::is_none")]
    pub parameter_list: Option<ParameterList>,

    /// Contains parameter templates (EPSG 311, 7.4.7.6).
    #[serde(rename = "templateList", default, skip_serializing_if = "Option::is_none")]
    pub template_list: Option<TemplateList>,
    
    // Other lists like dataTypeList, functionTypeList, etc., can be added here if needed.
    #[serde(rename = "dataTypeList", default, skip_serializing_if = "Option::is_none")]
    pub data_type_list: Option<AppDataTypeList>,
}

/// Represents `<templateList>` (EPSG 311, 7.4.7.6).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct TemplateList {
    #[serde(rename = "parameterTemplate", default, skip_serializing_if = "Vec::is_empty")]
    pub parameter_template: Vec<Parameter>, // Use Parameter struct, as it's identical
    
    #[serde(rename = "allowedValuesTemplate", default, skip_serializing_if = "Vec::is_empty")]
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
    #[serde(rename = "@uniqueIDRef", default, skip_serializing_if = "Option::is_none")]
    pub unique_id_ref: Option<String>,
    #[serde(rename = "@index", default, skip_serializing_if = "Option::is_none")]
    pub index: Option<i64>,
}

/// Represents the choice of data type for a Parameter.
/// (Fix for serde `flatten` on enum variant error)
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ParameterDataType {
    // Variants from GSimple (inlined)
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
    
    // Other choices
    #[serde(rename = "dataTypeIDRef")] // Fix: Add rename attribute
    DataTypeIDRef(DataTypeIDRef),
    #[serde(rename = "variableRef")] // Fix: Add rename attribute
    VariableRef(VariableRef),
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
    #[serde(rename = "@templateIDRef", default, skip_serializing_if = "Option::is_none")]
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
    #[serde(rename = "@multiplier", default, skip_serializing_if = "Option::is_none")]
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
/// (Updated for Task 5)
#[derive(Debug, Serialize, Deserialize)]
pub struct Parameter {
    /// The unique ID (e.g., "Param1_Vendor_Specific") that `uniqueIDRef` points to.
    #[serde(rename = "@uniqueID")]
    pub unique_id: String,

    #[serde(rename = "@access", default, skip_serializing_if = "Option::is_none")]
    pub access: Option<ParameterAccess>,

    #[serde(rename = "@support", default, skip_serializing_if = "Option::is_none")]
    pub support: Option<ParameterSupport>,
    
    #[serde(rename = "@persistent", default, skip_serializing_if = "is_false")]
    pub persistent: bool,
    
    #[serde(rename = "@offset", default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<String>,

    #[serde(rename = "@multiplier", default, skip_serializing_if = "Option::is_none")]
    pub multiplier: Option<String>,

    /// An optional reference to a `<parameterTemplate>`.
    #[serde(rename = "@templateIDRef", default, skip_serializing_if = "Option::is_none")]
    pub template_id_ref: Option<String>,
    
    // --- Elements ---
    
    #[serde(flatten)]
    pub labels: Glabels,
    
    #[serde(flatten)]
    pub data_type: ParameterDataType,
    
    #[serde(rename = "conditionalSupport", default, skip_serializing_if = "Vec::is_empty")]
    pub conditional_support: Vec<ConditionalSupport>,

    #[serde(rename = "denotation", default, skip_serializing_if = "Option::is_none")]
    pub denotation: Option<Denotation>,
    
    /// The `actualValue` element, if present (less common for defaults).
    #[serde(rename = "actualValue", default, skip_serializing_if = "Option::is_none")]
    pub actual_value: Option<Value>,

    /// The `defaultValue` element, if present.
    #[serde(rename = "defaultValue", default, skip_serializing_if = "Option::is_none")]
    pub default_value: Option<Value>,
    
    #[serde(rename = "substituteValue", default, skip_serializing_if = "Option::is_none")]
    pub substitute_value: Option<Value>,

    #[serde(rename = "allowedValues", default, skip_serializing_if = "Option::is_none")]
    pub allowed_values: Option<AllowedValues>,

    #[serde(rename = "unit", default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<Unit>,

    #[serde(rename = "property", default, skip_serializing_if = "Vec::is_empty")]
    pub property: Vec<Property>,
}

/// Represents a simple value-holding element like `<defaultValue value="0x01"/>`.
/// (EPSG 311, 7.4.7.7.2.4, 7.4.7.7.2.5).
/// (Updated for Task 5 to `t_value`)
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Value {
    #[serde(flatten, default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<Glabels>,
    
    #[serde(rename = "@value")]
    pub value: String,

    #[serde(rename = "@offset", default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<String>,

    #[serde(rename = "@multiplier", default, skip_serializing_if = "Option::is_none")]
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

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct AppArray {
    // Stub for future implementation
    #[serde(rename = "@name")]
    pub name: String,
}
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct AppStruct {
    // Stub for future implementation
    #[serde(rename = "@name")]
    pub name: String,
}
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct AppEnum {
    // Stub for future implementation
    #[serde(rename = "@name")]
    pub name: String,
}
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct AppDerived {
    // Stub for future implementation
    #[serde(rename = "@name")]
    pub name: String,
}