// crates/powerlink-rs-xdc/src/model.rs

//! Internal `serde` data structures that map directly to the XDC XML schema.
//! These are used for raw deserialization and serialization.

#![allow(clippy::pedantic)] // XML schema names are not idiomatic Rust

use serde::{Deserialize, Serialize};
use alloc::vec::Vec;
use alloc::string::String;

/// The root element of an XDC/XDD file.
/// (Based on ISO 15745-1:2005/Amd.1)
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename = "ISO15745ProfileContainer")]
pub struct Iso15745ProfileContainer {
    #[serde(rename = "@xmlns")]
    pub xmlns: String,

    #[serde(rename = "@xmlns:xsi")]
    pub xmlns_xsi: String,

    #[serde(rename = "@xsi:schemaLocation")]
    pub xsi_schema_location: String,

    #[serde(rename = "ISO15745Profile", default)]
    pub profile: Vec<Iso15745Profile>,
}

impl Default for Iso15745ProfileContainer {
    fn default() -> Self {
        Self {
            xmlns: "http://www.ethernet-powerlink.org".into(),
            xmlns_xsi: "http://www.w3.org/2001/XMLSchema-instance".into(),
            xsi_schema_location: "http://www.ethernet-powerlink.org Powerlink_Main.xsd".into(),
            profile: Vec::new(),
        }
    }
}

/// Represents either the Device Profile or the Communication Network Profile.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Iso15745Profile {
    #[serde(rename = "ProfileHeader")]
    pub profile_header: ProfileHeader,
    
    #[serde(rename = "ProfileBody")]
    pub profile_body: ProfileBody,
}

/// Header containing metadata. We don't parse its contents yet, but it must
/// be present for serde to succeed.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ProfileHeader {
    // We can add fields here if we need them later (e.g., ProfileIdentification)
}

/// The main body containing either device or communication data.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ProfileBody {
    /// Used to identify which ProfileBody this is (e.g. "ProfileBody_Device_Powerlink").
    #[serde(rename = "@xsi:type", default, skip_serializing_if = "Option::is_none")]
    pub xsi_type: Option<String>,

    /// This field is only present in the Communication Network Profile.
    #[serde(rename = "ApplicationLayers", default, skip_serializing_if = "Option::is_none")]
    pub application_layers: Option<ApplicationLayers>,

    /// This field is only present in the Device Profile.
    #[serde(rename = "DeviceIdentity", default, skip_serializing_if = "Option::is_none")]
    pub device_identity: Option<DeviceIdentity>,

    /// This field is only present in the Device Profile.
    #[serde(rename = "ApplicationProcess", default, skip_serializing_if = "Option::is_none")]
    pub application_process: Option<ApplicationProcess>,
}

/// Represents the `<DeviceIdentity>` block in the Device Profile.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct DeviceIdentity {
    #[serde(rename = "vendorName", default, skip_serializing_if = "Option::is_none")]
    pub vendor_name: Option<String>,

    #[serde(rename = "vendorID", default, skip_serializing_if = "Option::is_none")]
    pub vendor_id: Option<String>, // e.g., "0x12345678"

    #[serde(rename = "productName", default, skip_serializing_if = "Option::is_none")]
    pub product_name: Option<String>, // e.g., "MyName"

    #[serde(rename = "productID", default, skip_serializing_if = "Option::is_none")]
    pub product_id: Option<String>, // e.g., "1234"
    
    #[serde(rename = "version", default, skip_serializing_if = "Vec::is_empty")]
    pub version: Vec<Version>, // e.g., <version versionType="HW" value="1" />
}

/// Represents a `<version>` element within `<DeviceIdentity>`.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Version {
    #[serde(rename = "@versionType")]
    pub version_type: String,

    #[serde(rename = "@value")]
    pub value: String,
}

/// Contains the ObjectList and DataTypeList.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ApplicationLayers {
    /// This optional list defines the mapping from hex ID to type name.
    /// (EPSG 311, 7.5.4.3)
    #[serde(rename = "DataTypeList", default, skip_serializing_if = "Option::is_none")]
    pub data_type_list: Option<DataTypeList>,

    #[serde(rename = "ObjectList")]
    pub object_list: ObjectList,
}

/// A list of all Object Dictionary entries.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ObjectList {
    #[serde(rename = "Object", default)]
    pub object: Vec<Object>,
}

// --- Enums for Object/SubObject Attributes ---

/// Access types of an object / subobject (from XSD `t_ObjectAccessType`).
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum ObjectAccessType {
    #[serde(rename = "ro")]
    ReadOnly,
    #[serde(rename = "wo")]
    WriteOnly,
    #[serde(rename = "rw")]
    ReadWrite,
    #[serde(rename = "const")]
    Constant,
}

/// Ability to map an object / subobject to a PDO (from XSD `t_ObjectPDOMapping`).
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum ObjectPdoMapping {
    #[serde(rename = "no")]
    No,
    #[serde(rename = "default")]
    Default,
    #[serde(rename = "optional")]
    Optional,
    #[serde(rename = "TPDO")]
    Tpdo,
    #[serde(rename = "RPDO")]
    Rpdo,
}

/// Represents an Object Dictionary index (e.g., <Object index="1F22"...>).
/// This struct includes attributes from the `ag_Powerlink_Object` group.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Object {
    /// The OD index as a hex string (e.g., "1F22").
    #[serde(rename = "@index")]
    pub index: String,
    
    // --- Fields from ag_Powerlink_Object ---
    
    /// The name of the object.
    #[serde(rename = "@name")]
    pub name: String,

    /// The object type (e.g., "9" for RECORD).
    #[serde(rename = "@objectType")]
    pub object_type: String,

    /// The POWERLINK data type (e.g., "0006" for Unsigned16).
    #[serde(rename = "@dataType", default, skip_serializing_if = "Option::is_none")]
    pub data_type: Option<String>,

    /// The lower limit of the object's value.
    #[serde(rename = "@lowLimit", default, skip_serializing_if = "Option::is_none")]
    pub low_limit: Option<String>,

    /// The upper limit of the object's value.
    #[serde(rename = "@highLimit", default, skip_serializing_if = "Option::is_none")]
    pub high_limit: Option<String>,

    /// The access type (e.g., "ro", "rw").
    #[serde(rename = "@accessType", default, skip_serializing_if = "Option::is_none")]
    pub access_type: Option<ObjectAccessType>,

    /// The default value of the object.
    #[serde(rename = "@defaultValue", default, skip_serializing_if = "Option::is_none")]
    pub default_value: Option<String>,

    /// The actual value of the object (used in XDC).
    #[serde(rename = "@actualValue", default, skip_serializing_if = "Option::is_none")]
    pub actual_value: Option<String>,

    /// A denotation for the object.
    #[serde(rename = "@denotation", default, skip_serializing_if = "Option::is_none")]
    pub denotation: Option<String>,

    /// The PDO mapping capability of the object.
    #[serde(rename = "@PDOmapping", default, skip_serializing_if = "Option::is_none")]
    pub pdo_mapping: Option<ObjectPdoMapping>,

    /// Object flags.
    #[serde(rename = "@objFlags", default, skip_serializing_if = "Option::is_none")]
    pub obj_flags: Option<String>,

    /// This attribute references a Parameter's uniqueID in the ApplicationProcess.
    #[serde(rename = "@uniqueIDRef", default, skip_serializing_if = "Option::is_none")]
    pub unique_id_ref: Option<String>,
    
    // --- End of fields from ag_Powerlink_Object ---

    /// A list of SubObjects (e.g., <SubObject subIndex="01"...>).
    #[serde(rename = "SubObject", default)]
    pub sub_object: Vec<SubObject>,
}

/// Represents an Object Dictionary sub-index.
/// This struct includes attributes from the `ag_Powerlink_Object` group.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct SubObject {
    /// The OD sub-index as a hex string (e.g., "01").
    #[serde(rename = "@subIndex")]
    pub sub_index: String,
    
    // --- Fields from ag_Powerlink_Object ---

    /// The name of the sub-object.
    #[serde(rename = "@name")]
    pub name: String,

    /// The object type (e.g., "7" for VAR).
    #[serde(rename = "@objectType")]
    pub object_type: String,

    /// The POWERLINK data type (e.g., "0006" for Unsigned16).
    #[serde(rename = "@dataType", default, skip_serializing_if = "Option::is_none")]
    pub data_type: Option<String>,

    /// The lower limit of the sub-object's value.
    #[serde(rename = "@lowLimit", default, skip_serializing_if = "Option::is_none")]
    pub low_limit: Option<String>,

    /// The upper limit of the sub-object's value.
    #[serde(rename = "@highLimit", default, skip_serializing_if = "Option::is_none")]
    pub high_limit: Option<String>,

    /// The access type (e.g., "ro", "rw").
    #[serde(rename = "@accessType", default, skip_serializing_if = "Option::is_none")]
    pub access_type: Option<ObjectAccessType>,

    /// The `defaultValue` is the key data for an XDD file.
    #[serde(rename = "@defaultValue", default, skip_serializing_if = "Option::is_none")]
    pub default_value: Option<String>,

    /// The `actualValue` is the key data for an XDC file.
    #[serde(rename = "@actualValue", default, skip_serializing_if = "Option::is_none")]
    pub actual_value: Option<String>,

    /// A denotation for the sub-object.
    #[serde(rename = "@denotation", default, skip_serializing_if = "Option::is_none")]
    pub denotation: Option<String>,

    /// The PDO mapping capability of the sub-object.
    #[serde(rename = "@PDOmapping", default, skip_serializing_if = "Option::is_none")]
    pub pdo_mapping: Option<ObjectPdoMapping>,

    /// Object flags.
    #[serde(rename = "@objFlags", default, skip_serializing_if = "Option::is_none")]
    pub obj_flags: Option<String>,

    /// This attribute references a Parameter's uniqueID in the ApplicationProcess.
    #[serde(rename = "@uniqueIDRef", default, skip_serializing_if = "Option::is_none")]
    pub unique_id_ref: Option<String>,

    // --- End of fields from ag_Powerlink_Object ---
}

// --- STRUCTS for DataTypeList (Comm Profile) ---

/// Represents `<DataTypeList>` (EPSG 311, 7.5.4.3).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct DataTypeList {
    #[serde(rename = "defType", default, skip_serializing_if = "Vec::is_empty")]
    pub def_type: Vec<DefType>,
}

/// Represents `<defType>` (EPSG 311, 7.5.4.3).
#[derive(Debug, Serialize, Deserialize)]
pub struct DefType {
    /// The hex ID for the data type (e.g., "0006").
    #[serde(rename = "@dataType")]
    pub data_type: String,

    /// This captures the name of the child element (e.g., `<Unsigned16/>`).
    #[serde(rename = "$value")]
    pub type_name: DataTypeName,
}

/// Represents the tag name of the child of `<defType>`.
/// (Based on EPSG 311, Table 56).
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum DataTypeName {
    Boolean,
    Integer8,
    Integer16,
    Integer32,
    Unsigned8,
    Unsigned16,
    Unsigned32,
    Real32,
    #[serde(rename = "Visible_String")]
    VisibleString,
    Integer24,
    Real64,
    Integer40,
    Integer48,
    Integer56,
    Integer64,
    #[serde(rename = "Octet_String")]
    OctetString,
    #[serde(rename = "Unicode_String")]
    UnicodeString,
    #[serde(rename = "Time_of_Day")]
    TimeOfDay,
    #[serde(rename = "Time_Diff")]
    TimeDiff,
    Domain,
    Unsigned24,
    Unsigned40,
    Unsigned48,
    Unsigned56,
    Unsigned64,
    #[serde(rename = "MAC_ADDRESS")]
    MacAddress,
    #[serde(rename = "IP_ADDRESS")]
    IpAddress,
    NETTIME,
}

// --- STRUCTS for ApplicationProcess ---

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
}

/// Represents `<templateList>` (EPSG 311, 7.4.7.6).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct TemplateList {
    #[serde(rename = "parameterTemplate", default, skip_serializing_if = "Vec::is_empty")]
    pub parameter_template: Vec<ParameterTemplate>,
}

/// Represents `<parameterTemplate>` (EPSG 311, 7.4.7.6).
/// This is a simplified version, only capturing what we need for default value resolution.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ParameterTemplate {
    /// The unique ID (e.g., "Template_U16") that `templateIDRef` points to.
    #[serde(rename = "@uniqueID")]
    pub unique_id: String,

    /// The `defaultValue` element, if present.
    #[serde(rename = "defaultValue", default, skip_serializing_if = "Option::is_none")]
    pub default_value: Option<Value>,

    /// The `actualValue` element, if present.
    #[serde(rename = "actualValue", default, skip_serializing_if = "Option::is_none")]
    pub actual_value: Option<Value>,
}

/// Represents `<parameterList>` (EPSG 311, 7.4.7.7).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ParameterList {
    #[serde(rename = "parameter", default, skip_serializing_if = "Vec::is_empty")]
    pub parameter: Vec<Parameter>,
}

/// Represents a `<parameter>` (EPSG 311, 7.4.7.7.2).
/// We only capture the fields relevant for default value resolution.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Parameter {
    /// The unique ID (e.g., "Param1_Vendor_Specific") that `uniqueIDRef` points to.
    #[serde(rename = "@uniqueID")]
    pub unique_id: String,

    /// An optional reference to a `<parameterTemplate>`.
    #[serde(rename = "@templateIDRef", default, skip_serializing_if = "Option::is_none")]
    pub template_id_ref: Option<String>,
    
    /// The `defaultValue` element, if present.
    #[serde(rename = "defaultValue", default, skip_serializing_if = "Option::is_none")]
    pub default_value: Option<Value>,

    /// The `actualValue` element, if present (less common for defaults).
    #[serde(rename = "actualValue", default, skip_serializing_if = "Option::is_none")]
    pub actual_value: Option<Value>,
    
    // Other attributes like access, support, etc., can be added here.
}

/// Represents a simple value-holding element like `<defaultValue value="0x01"/>`.
/// (EPSG 311, 7.4.7.7.2.4, 7.4.7.7.2.5).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Value {
    #[serde(rename = "@value")]
    pub value: String,
}