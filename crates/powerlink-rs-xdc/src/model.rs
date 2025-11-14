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

// --- Enums and Structs for ProfileHeader ---

/// Represents the `<ProfileClassID>` element (from XSD `t_ProfileClassID`).
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum ProfileClassId {
    DeviceProfile,
    CommunicationNetworkProfile,
    ApplicationLayerProfile,
}

impl Default for ProfileClassId {
    fn default() -> Self {
        // All fields in a struct with `#[derive(Default)]` must implement
        // Default. `ProfileHeader` derives Default. While there's no
        // "correct" default, `DeviceProfile` is the most common for XDC.
        Self::DeviceProfile
    }
}


/// Represents the `<ISO15745Reference>` element (from XSD `t_ISO15745Reference`).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Iso15745Reference {
    #[serde(rename = "ISO15745Part")]
    pub iso15745_part: u32,
    #[serde(rename = "ISO15745Edition")]
    pub iso15745_edition: u32,
    #[serde(rename = "ProfileTechnology")]
    pub profile_technology: String,
}

/// Represents the `<IASInterfaceType>` element wrapper (from XSD `t_IASInterfaceType`).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct IasInterfaceType {
    #[serde(rename = "IASInterfaceType")]
    pub ias_interface_type: String,
}

/// Represents `<ProfileHeader>` (from XSD `t_ProfileHeader`).
/// This contains metadata identifying the profile.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ProfileHeader {
    #[serde(rename = "ProfileIdentification")]
    pub profile_identification: String,
    
    #[serde(rename = "ProfileRevision")]
    pub profile_revision: String,

    #[serde(rename = "ProfileName")]
    pub profile_name: String,

    #[serde(rename = "ProfileSource")]
    pub profile_source: String,

    #[serde(rename = "ProfileClassID")]
    pub profile_class_id: ProfileClassId,

    /// Stored as a string as `xs:date` (e.g., "2024-01-01")
    #[serde(rename = "ProfileDate", default, skip_serializing_if = "Option::is_none")]
    pub profile_date: Option<String>,

    #[serde(rename = "AdditionalInformation", default, skip_serializing_if = "Option::is_none")]
    pub additional_information: Option<String>,
    
    #[serde(rename = "ISO15745Reference", default, skip_serializing_if = "Vec::is_empty")]
    pub iso15745_reference: Vec<Iso15745Reference>,
    
    #[serde(rename = "IASInterfaceType", default, skip_serializing_if = "Option::is_none")]
    pub ias_interface_type: Option<IasInterfaceType>,
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

    /// This field is only present in the Communication Network Profile.
    #[serde(rename = "NetworkManagement", default, skip_serializing_if = "Option::is_none")]
    pub network_management: Option<NetworkManagement>,
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

// --- Structs for NetworkManagement (Comm Profile) ---

/// Represents `<NetworkManagement>` (from XSD `t_NetworkManagement`).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct NetworkManagement {
    #[serde(rename = "GeneralFeatures")]
    pub general_features: GeneralFeatures,
    
    #[serde(rename = "MNFeatures", default, skip_serializing_if = "Option::is_none")]
    pub mn_features: Option<MnFeatures>,
    
    #[serde(rename = "CNFeatures", default, skip_serializing_if = "Option::is_none")]
    pub cn_features: Option<CnFeatures>,
    
    #[serde(rename = "deviceCommissioning", default, skip_serializing_if = "Option::is_none")]
    pub device_commissioning: Option<DeviceCommissioning>,
    
    #[serde(rename = "Diagnostic", default, skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<Diagnostic>,
}

/// Represents `<GeneralFeatures>` (from XSD `t_GeneralFeatures`).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct GeneralFeatures {
    #[serde(rename = "@DLLFeatureMN", default, skip_serializing_if = "Option::is_none")]
    pub dll_feature_mn: Option<bool>,
    
    #[serde(rename = "@NMTBootTimeNotActive", default, skip_serializing_if = "Option::is_none")]
    pub nmt_boot_time_not_active: Option<String>,
    
    // ... other GeneralFeatures attributes can be added here ...
}

/// Represents `<MNFeatures>` (from XSD `t_MNFeatures`).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct MnFeatures {
    #[serde(rename = "@NMTMNMaxCycInSync", default, skip_serializing_if = "Option::is_none")]
    pub nmt_mn_max_cyc_in_sync: Option<String>,
    
    #[serde(rename = "@NMTMNPResMax", default, skip_serializing_if = "Option::is_none")]
    pub nmt_mn_pres_max: Option<String>,
    
    // ... other MNFeatures attributes can be added here ...
}

/// Represents the `NMTCNDNA` attribute enum (from XSD `t_CNFeaturesNMT_CN_DNA`).
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum CnFeaturesNmtCnDna {
    /// "0" = Do not clear
    #[serde(rename = "0")]
    DoNotClear,
    /// "1" = Clear on PRE_OP1 -> PRE_OP2
    #[serde(rename = "1")]
    ClearOnPreOp1ToPreOp2,
    /// "2" = Clear on NMT_Reset_Node
    #[serde(rename = "2")]
    ClearOnNmtResetNode,
}

/// Represents `<CNFeatures>` (from XSD `t_CNFeatures`).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct CnFeatures {
    #[serde(rename = "@NMTCNPreOp2ToReady2Op", default, skip_serializing_if = "Option::is_none")]
    pub nmt_cn_pre_op2_to_ready2_op: Option<String>,
    
    #[serde(rename = "@NMTCNDNA", default, skip_serializing_if = "Option::is_none")]
    pub nmt_cn_dna: Option<CnFeaturesNmtCnDna>,
    
    // ... other CNFeatures attributes can be added here ...
}

/// Represents `<deviceCommissioning>` (from XSD `t_deviceCommissioning`).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct DeviceCommissioning {
    #[serde(rename = "@NMTNodeIDByHW", default)]
    pub nmt_node_id_by_hw: bool,

    #[serde(rename = "@NMTNodeIDBySW", default)]
    pub nmt_node_id_by_sw: bool,
    
    // ... other deviceCommissioning attributes can be added here ...
}

/// Represents `<Diagnostic>` (from XSD `t_Diagnostic`).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Diagnostic {
    #[serde(rename = "ErrorList", default, skip_serializing_if = "Option::is_none")]
    pub error_list: Option<ErrorList>,
}

/// Represents `<ErrorList>` (from XSD `t_ErrorList`).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ErrorList {
    #[serde(rename = "Error", default, skip_serializing_if = "Vec::is_empty")]
    pub error: Vec<Error>,
}

/// Represents `<Error>` (from XSD `t_Error`).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Error {
    #[serde(rename = "@name", default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    #[serde(rename = "@label", default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    
    #[serde(rename = "@description", default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    
    #[serde(rename = "@type", default, skip_serializing_if = "Option::is_none")]
    pub error_type: Option<String>,
    
    #[serde(rename = "@value", default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}