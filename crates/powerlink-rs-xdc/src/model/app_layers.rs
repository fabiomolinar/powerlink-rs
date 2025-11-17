// crates/powerlink-rs-xdc/src/model/app_layers.rs

//! Contains model structs related to `<ApplicationLayers>`.
//! (Schema: `ProfileBody_CommunicationNetwork_Powerlink.xsd`)

use super::modular::ModuleManagementComm;
use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize}; // Added import

/// Contains the ObjectList and DataTypeList.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ApplicationLayers {
    /// This optional list defines the mapping from hex ID to type name.
    /// (EPSG 311, 7.5.4.3)
    #[serde(
        rename = "DataTypeList",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub data_type_list: Option<DataTypeList>,

    #[serde(rename = "ObjectList")]
    pub object_list: ObjectList,

    /// This field is only present in Modular Head communication profiles.
    /// (from `ProfileBody_CommunicationNetwork_Powerlink_Modular_Head.xsd`)
    #[serde(
        rename = "moduleManagement",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub module_management: Option<ModuleManagementComm>,
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
    #[serde(
        rename = "@highLimit",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub high_limit: Option<String>,

    /// The access type (e.g., "ro", "rw").
    #[serde(
        rename = "@accessType",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub access_type: Option<ObjectAccessType>,

    /// The default value of the object.
    #[serde(
        rename = "@defaultValue",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub default_value: Option<String>,

    /// The actual value of the object (used in XDC).
    #[serde(
        rename = "@actualValue",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub actual_value: Option<String>,

    /// A denotation for the object.
    #[serde(
        rename = "@denotation",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub denotation: Option<String>,

    /// The PDO mapping capability of the object.
    #[serde(
        rename = "@PDOmapping",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub pdo_mapping: Option<ObjectPdoMapping>,

    /// Object flags.
    #[serde(rename = "@objFlags", default, skip_serializing_if = "Option::is_none")]
    pub obj_flags: Option<String>,

    /// This attribute references a Parameter's uniqueID in the ApplicationProcess.
    #[serde(
        rename = "@uniqueIDRef",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub unique_id_ref: Option<String>,

    // --- End of fields from ag_Powerlink_Object ---
    /// This attribute is used by modular devices to reference an index range.
    /// (from `t_Object_Extension_Head` and `t_Object_Extension`)
    #[serde(
        rename = "@rangeSelector",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub range_selector: Option<String>,

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

    /// The object type (e.t., "7" for VAR).
    #[serde(rename = "@objectType")]
    pub object_type: String,

    /// The POWERLINK data type (e.g., "0006" for Unsigned16).
    #[serde(rename = "@dataType", default, skip_serializing_if = "Option::is_none")]
    pub data_type: Option<String>,

    /// The lower limit of the sub-object's value.
    #[serde(rename = "@lowLimit", default, skip_serializing_if = "Option::is_none")]
    pub low_limit: Option<String>,

    /// The upper limit of the sub-object's value.
    #[serde(
        rename = "@highLimit",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub high_limit: Option<String>,

    /// The access type (e.g., "ro", "rw").
    #[serde(
        rename = "@accessType",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub access_type: Option<ObjectAccessType>,

    /// The `defaultValue` is the key data for an XDD file.
    #[serde(
        rename = "@defaultValue",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub default_value: Option<String>,

    /// The `actualValue` is the key data for an XDC file.
    #[serde(
        rename = "@actualValue",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub actual_value: Option<String>,

    /// A denotation for the sub-object.
    #[serde(
        rename = "@denotation",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub denotation: Option<String>,

    /// The PDO mapping capability of the sub-object.
    #[serde(
        rename = "@PDOmapping",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub pdo_mapping: Option<ObjectPdoMapping>,

    /// Object flags.
    #[serde(rename = "@objFlags", default, skip_serializing_if = "Option::is_none")]
    pub obj_flags: Option<String>,

    /// This attribute references a Parameter's uniqueID in the ApplicationProcess.
    #[serde(
        rename = "@uniqueIDRef",
        default,
        skip_serializing_if = "Option::is_none"
    )]
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
