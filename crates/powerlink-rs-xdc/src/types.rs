// crates/powerlink-rs-xdc/src/types.rs

//! Public, ergonomic data structures for representing a parsed XDC file.

use alloc::string::String;
use alloc::vec::Vec;

// --- Root XDC Structure ---

/// Represents a fully parsed and resolved XDC/XDD file.
///
/// This is the main public struct, providing ergonomic access to all
/// data contained within the XML file.
#[derive(Debug, Default)]
pub struct XdcFile {
    /// Metadata from the `<ProfileHeader>` block.
    pub header: ProfileHeader,
    
    /// Information from the `<DeviceIdentity>` block.
    pub identity: Identity,
    
    /// Information from the `<NetworkManagement>` block.
    /// This is `None` if the file is a standard Device Profile (XDD).
    pub network_management: Option<NetworkManagement>,

    /// Information from the `<ApplicationProcess>` block.
    pub application_process: Option<ApplicationProcess>,
    
    /// The complete Object Dictionary for the device.
    pub object_dictionary: ObjectDictionary,
    
    // We can add ApplicationProcess here later if needed.
}

// --- Profile Header ---

/// Represents the `<ProfileHeader>` block, containing file metadata.
#[derive(Debug, Default)]
pub struct ProfileHeader {
    /// `<ProfileIdentification>`
    pub identification: String,
    /// `<ProfileRevision>`
    pub revision: String,
    /// `<ProfileName>`
    pub name: String,
    /// `<ProfileSource>`
    pub source: String,
    /// `<ProfileDate>`
    pub date: Option<String>,
}

// --- Device Identity ---

/// Represents the `<DeviceIdentity>` block.
/// (Updated for Task 3)
#[derive(Debug, Default)]
pub struct Identity {
    /// `<vendorName>` (Mandatory)
    pub vendor_name: String,
    /// `<vendorID>` (as a u32, parsed from hex)
    pub vendor_id: u32,
    /// `<vendorText>` (First available label)
    pub vendor_text: Option<String>,
    
    /// `<deviceFamily>` (First available label)
    pub device_family: Option<String>,
    /// `<productFamily>`
    pub product_family: Option<String>,
    
    /// `<productName>` (Mandatory)
    pub product_name: String,
    /// `<productID>` (as a u32, parsed from hex)
    pub product_id: u32,
    /// `<productText>` (First available label)
    pub product_text: Option<String>,
    
    /// All `<orderNumber>` elements.
    pub order_number: Vec<String>,
    /// All `<version>` elements.
    pub versions: Vec<Version>,
    
    /// `<buildDate>`
    pub build_date: Option<String>,
    /// `<specificationRevision>`
    pub specification_revision: Option<String>,
    /// `<instanceName>`
    pub instance_name: Option<String>,
}

/// Represents a `<version>` element.
#[derive(Debug, Default)]
pub struct Version {
    /// `@versionType`
    pub version_type: String,
    /// `@value`
    pub value: String,
}

// --- Network Management ---

/// Represents the `<NetworkManagement>` block from the Comm Profile.
#[derive(Debug, Default)]
pub struct NetworkManagement {
    pub general_features: GeneralFeatures,
    pub mn_features: Option<MnFeatures>,
    pub cn_features: Option<CnFeatures>,
    pub diagnostic: Option<Diagnostic>,
    // Add deviceCommissioning later if needed
}

/// Represents `<GeneralFeatures>`.
#[derive(Debug, Default)]
pub struct GeneralFeatures {
    /// `@DLLFeatureMN`
    pub dll_feature_mn: bool,
    /// `@NMTBootTimeNotActive` (in microseconds)
    pub nmt_boot_time_not_active: u32,
    /// `@NMTCycleTimeMax` (in microseconds)
    pub nmt_cycle_time_max: u32,
    /// `@NMTCycleTimeMin` (in microseconds)
    pub nmt_cycle_time_min: u32,
    /// `@NMTErrorEntries`
    pub nmt_error_entries: u32,
    /// `@NMTMaxCNNumber`
    pub nmt_max_cn_number: Option<u8>,
    /// `@PDODynamicMapping`
    pub pdo_dynamic_mapping: Option<bool>,
    /// `@SDOClient`
    pub sdo_client: Option<bool>,
    /// `@SDOServer`
    pub sdo_server: Option<bool>,
    /// `@SDOSupportASnd`
    pub sdo_support_asnd: Option<bool>,
    /// `@SDOSupportUdpIp`
    pub sdo_support_udp_ip: Option<bool>,
}

/// Represents `<MNFeatures>`.
#[derive(Debug, Default)]
pub struct MnFeatures {
    /// `@DLLMNFeatureMultiplex`
    pub dll_mn_feature_multiplex: Option<bool>,
    /// `@NMTMNPResChaining`
    pub dll_mn_pres_chaining: Option<bool>,
    /// `@NMTSimpleBoot`
    pub nmt_simple_boot: bool,
}

/// Public representation of the `@NMTCNDNA` attribute (Dynamic Node Addressing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NmtCnDna {
    /// "0" = Do not clear.
    DoNotClear,
    /// "1" = Clear on PRE_OP1 -> PRE_OP2.
    ClearOnPreOp1ToPreOp2,
    /// "2" = Clear on NMT_Reset_Node.
    ClearOnNmtResetNode,
}

/// Represents `<CNFeatures>`.
#[derive(Debug, Default)]
pub struct CnFeatures {
    /// `@DLLCNFeatureMultiplex`
    pub dll_cn_feature_multiplex: Option<bool>,
    /// `@DLLCNPResChaining`
    pub dll_cn_pres_chaining: Option<bool>,
    /// `@NMTCNPreOp2ToReady2Op` (in nanoseconds)
    pub nmt_cn_pre_op2_to_ready2_op: Option<u32>,
    /// `@NMTCNSoC2PReq` (in nanoseconds)
    pub nmt_cn_soc_2_preq: u32,
    /// `@NMTCNDNA`
    pub nmt_cn_dna: Option<NmtCnDna>, // Changed from Option<bool>
}

/// Represents `<Diagnostic>` capabilities.
#[derive(Debug, Default)]
pub struct Diagnostic {
    /// All defined `<Error>` elements.
    pub errors: Vec<ErrorDefinition>,
    /// All defined `<ErrorBit>` elements from `<StaticErrorBitField>`.
    pub static_error_bit_field: Option<Vec<StaticErrorBit>>,
}

/// Represents one `<Error>` in the `<ErrorList>`.
#[derive(Debug, Default)]
pub struct ErrorDefinition {
    pub name: String,
    pub value: String, 
    pub add_info: Vec<AddInfo>,
}

/// Represents one `<addInfo>` element from an `<Error>`.
#[derive(Debug, Default)]
pub struct AddInfo {
    pub name: String,
    pub bit_offset: u8,
    pub len: u8,
    pub description: Option<String>,
}

/// Represents one `<ErrorBit>` from the `<StaticErrorBitField>`.
#[derive(Debug, Default)]
pub struct StaticErrorBit {
    pub name: String,
    pub offset: u8,
    pub label: Option<String>,
    pub description: Option<String>,
}

// --- Application Process ---

/// Represents the `<ApplicationProcess>` block, containing user-defined
/// data types, parameters, and groupings.
#[derive(Debug, Default)]
pub struct ApplicationProcess {
    /// List of user-defined data types.
    pub data_types: Vec<AppDataType>,
    /// List of parameter groups.
    pub parameter_groups: Vec<ParameterGroup>,
    // TODO: Add FunctionTypeList and FunctionInstanceList
}

/// An enum representing a user-defined data type from `<dataTypeList>`.
#[derive(Debug)]
pub enum AppDataType {
    Struct(AppStruct),
    Array(AppArray),
    Enum(AppEnum),
    Derived(AppDerived),
}

/// Represents a `<struct>` data type.
#[derive(Debug, Default)]
pub struct AppStruct {
    pub name: String,
    pub unique_id: String,
    pub label: Option<String>,
    pub description: Option<String>,
    pub members: Vec<StructMember>,
}

/// Represents a `<varDeclaration>` within a `<struct>`.
#[derive(Debug, Default)]
pub struct StructMember {
    pub name: String,
    pub unique_id: String,
    /// The data type of this member (e.g., "UINT", "BOOL", or a `uniqueIDRef`
    /// to another type in the `dataTypeList`).
    pub data_type: String,
    /// Size in bits, if applicable (e.g., for `BITSTRING`).
    pub size: Option<u32>,
    pub label: Option<String>,
    pub description: Option<String>,
}

/// Represents an `<array>` data type.
#[derive(Debug, Default)]
pub struct AppArray {
    pub name: String,
    pub unique_id: String,
    pub label: Option<String>,
    pub description: Option<String>,
    pub lower_limit: u32,
    pub upper_limit: u32,
    /// The data type of the array elements (e.g., "UINT", "BOOL", or a `uniqueIDRef`).
    pub data_type: String,
}

/// Represents an `<enum>` data type.
#[derive(Debug, Default)]
pub struct AppEnum {
    pub name: String,
    pub unique_id: String,
    pub label: Option<String>,
    pub description: Option<String>,
    /// The base data type for the enum (e.g., "USINT", "UINT").
    pub data_type: String,
    pub size_in_bits: Option<u32>,
    pub values: Vec<EnumValue>,
}

/// Represents a single `<enumValue>` within an `<enum>`.
#[derive(Debug, Default)]
pub struct EnumValue {
    pub name: String,
    pub value: String,
    pub label: Option<String>,
    pub description: Option<String>,
}

/// Represents a `<derived>` data type.
#[derive(Debug, Default)]
pub struct AppDerived {
    pub name: String,
    pub unique_id: String,
    pub label: Option<String>,
    pub description: Option<String>,
    /// The base data type this is derived from (e.g., "UINT", "BOOL", or a `uniqueIDRef`).
    pub data_type: String,
    pub count: Option<Count>,
}

/// Represents a `<count>` element within a `<derived>` type.
#[derive(Debug, Default)]
pub struct Count {
    pub unique_id: String,
    pub access: Option<ParameterAccess>,
    pub default_value: Option<String>,
}

/// Represents a `<parameterGroup>` from the `<parameterGroupList>`.
#[derive(Debug, Default)]
pub struct ParameterGroup {
    pub unique_id: String,
    pub label: Option<String>,
    pub description: Option<String>,
    /// Nested groups or parameter references.
    pub items: Vec<ParameterGroupItem>,
}

/// An enum representing an item inside a `<parameterGroup>`.
#[derive(Debug)]
pub enum ParameterGroupItem {
    /// A nested parameter group.
    Group(ParameterGroup),
    /// A reference to a parameter.
    Parameter(ParameterRef),
}

/// Represents a `<parameterRef>` inside a `<parameterGroup>`.
#[derive(Debug, Default)]
pub struct ParameterRef {
    /// The `uniqueID` of the parameter being referenced.
    pub unique_id_ref: String,
    pub visible: bool,
    pub locked: bool,
    /// Optional bit offset for bit-packed groups.
    pub bit_offset: Option<u32>,
}

// --- Object Dictionary ---

/// Access types for an Object Dictionary entry, resolved from either
/// `<Object @accessType>` or `<parameter @access>`.
/// (Based on XSD `t_parameter` access attribute, EPSG DS 311, Table 40)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParameterAccess {
    /// `const`
    Constant,
    /// `read`
    ReadOnly,
    /// `write`
    WriteOnly,
    /// `readWrite`
    ReadWrite,
    /// `readWriteInput`
    ReadWriteInput,
    /// `readWriteOutput`
    ReadWriteOutput,
    /// `noAccess`
    NoAccess,
}

/// Support level for an Object Dictionary entry, resolved from
/// `<parameter @support>`.
/// (Based on XSD `t_parameter` support attribute, EPSG DS 311, Table 40)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParameterSupport {
    /// `mandatory`
    Mandatory,
    /// `optional`
    Optional,
    /// `conditional`
    Conditional,
}

/// PDO mapping capabilities for an Object Dictionary entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectPdoMapping {
    No,
    Default,
    Optional,
    Tpdo,
    Rpdo,
}

/// Represents the complete `<ObjectList>` (Object Dictionary).
#[derive(Debug, Default)]
pub struct ObjectDictionary {
    pub objects: Vec<Object>,
}

/// Represents a single `<Object>` (an OD Index).
#[derive(Debug, Default)]
pub struct Object {
    /// `@index` (as a u16, parsed from hex)
    pub index: u16,
    
    // --- Metadata ---
    /// `@name`
    pub name: String,
    /// `@objectType` (e.g., "7" for VAR, "9" for RECORD)
    pub object_type: String,
    /// `@dataType` (as a hex string, e.g., "0006")
    pub data_type: Option<String>,
    /// `@lowLimit`
    pub low_limit: Option<String>,
    /// `@highLimit`
    pub high_limit: Option<String>,
    /// Resolved access type from `<Object @accessType>` or `<parameter @access>`.
    pub access_type: Option<ParameterAccess>,
    /// `@PDOmapping`
    pub pdo_mapping: Option<ObjectPdoMapping>,
    /// `@objFlags`
    pub obj_flags: Option<String>,
    /// Resolved support level from `<parameter @support>`.
    pub support: Option<ParameterSupport>,
    /// Resolved `persistent` flag from `<parameter @persistent>`.
    pub persistent: bool,
    
    // --- Value ---
    /// The resolved data for this object, from `actualValue` or `defaultValue`.
    /// This is `None` for RECORD types (where data is in sub-objects).
    pub data: Option<Vec<u8>>,
    
    // --- Children ---
    /// All `<SubObject>` children.
    pub sub_objects: Vec<SubObject>,
}

/// Represents a `<SubObject>` (an OD Sub-Index).
#[derive(Debug, Default)]
pub struct SubObject {
    /// `@subIndex` (as a u8, parsed from hex)
    pub sub_index: u8,
    
    // --- Metadata ---
    /// `@name`
    pub name: String,
    /// `@objectType` (e.D., "7" for VAR)
    pub object_type: String,
    /// `@dataType` (as a hex string, e.g., "0006")
    pub data_type: Option<String>,
    /// `@lowLimit`
    pub low_limit: Option<String>,
    /// `@highLimit`
    pub high_limit: Option<String>,
    /// Resolved access type from `<SubObject @accessType>` or `<parameter @access>`.
    pub access_type: Option<ParameterAccess>,
    /// `@PDOmapping`
    pub pdo_mapping: Option<ObjectPdoMapping>,
    /// `@objFlags`
    pub obj_flags: Option<String>,
    /// Resolved support level from `<parameter @support>`.
    pub support: Option<ParameterSupport>,
    /// Resolved `persistent` flag from `<parameter @persistent>`.
    pub persistent: bool,
    
    // --- Value ---
    /// The resolved data for this sub-object, from `actualValue` or `defaultValue`.
    pub data: Option<Vec<u8>>,
}