// crates/powerlink-rs-xdc/src/types.rs

//! Public, ergonomic data structures for representing a parsed XDC file.

use alloc::string::String;
use alloc::vec::Vec;

// --- Root XDC Structure ---

/// Represents a fully parsed and resolved XDC/XDD file.
///
/// This is the main public struct, providing ergonomic access to all
/// data contained within the XML file.
#[derive(Debug, Default, PartialEq)]
pub struct XdcFile {
    /// Metadata from the `<ProfileHeader>` block.
    pub header: ProfileHeader,
    
    /// Information from the `<DeviceIdentity>` block.
    pub identity: Identity,
    
    /// Information from the `<DeviceManager>` block.
    pub device_manager: Option<DeviceManager>,

    /// Information from the `<NetworkManagement>` block.
    /// This is `None` if the file is a standard Device Profile (XDD).
    pub network_management: Option<NetworkManagement>,

    /// Information from the `<ApplicationProcess>` block.
    pub application_process: Option<ApplicationProcess>,
    
    /// The complete Object Dictionary for the device.
    pub object_dictionary: ObjectDictionary,
    
    /// Information from the `<moduleManagement>` block in the *Communication Profile*.
    /// This defines the OD index ranges for modular devices.
    pub module_management_comm: Option<ModuleManagementComm>,
}

// --- Profile Header ---

/// Represents the `<ProfileHeader>` block, containing file metadata.
#[derive(Debug, Default, PartialEq)]
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
#[derive(Debug, Default, PartialEq)]
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
#[derive(Debug, Default, Clone, PartialEq)] // Added PartialEq
pub struct Version {
    /// `@versionType`
    pub version_type: String,
    /// `@value`
    pub value: String,
}

// --- Device Manager (New) ---

/// Represents the `<DeviceManager>` block.
#[derive(Debug, Default, PartialEq)]
pub struct DeviceManager {
    /// Contains information about device indicators, primarily LEDs.
    pub indicator_list: Option<IndicatorList>,
    /// Contains information about modular device interfaces (if applicable).
    pub module_management: Option<ModuleManagementDevice>,
}

/// Represents an `<indicatorList>` containing an `<LEDList>`.
#[derive(Debug, Default, PartialEq)]
pub struct IndicatorList {
    /// A list of all LEDs defined for the device.
    pub leds: Vec<LED>,
    /// A list of states defined by a combination of multiple LEDs.
    pub combined_states: Vec<CombinedState>,
}

/// Represents a single `<LED>` indicator.
#[derive(Debug, Default, PartialEq)]
pub struct LED {
    /// Primary label for the LED (e.g., "STATUS").
    pub label: Option<String>,
    /// Description of the LED's purpose.
    pub description: Option<String>,
    /// Whether the LED is "monocolor" or "bicolor".
    pub colors: String, // Mapped from `LEDcolors` enum
    /// The type of functionality the LED indicates ("IO", "device", "communication").
    pub led_type: Option<String>, // Mapped from `LEDtype` enum
    /// A list of all defined states for this LED.
    pub states: Vec<LEDstate>,
}

/// Represents a single `<LEDstate>` for a specific `<LED>`.
#[derive(Debug, Default, PartialEq)]
pub struct LEDstate {
    /// The unique ID used to reference this state (e.g., in `<combinedState>`).
    pub unique_id: String,
    /// The state being represented ("on", "off", "flashing").
    pub state: String, // Mapped from `LEDstateEnum`
    /// The color of the LED in this state ("green", "amber", "red").
    pub color: String, // Mapped from `LEDcolor`
    /// Primary label for this state.
    pub label: Option<String>,
    /// Description of what this state means.
    pub description: Option<String>,
}

/// Represents a `<combinedState>` that references multiple `<LEDstate>`s.
#[derive(Debug, Default, PartialEq)]
pub struct CombinedState {
    /// Primary label for this combined state.
    pub label: Option<String>,
    /// Description of what this combined state means.
    pub description: Option<String>,
    /// A list of `uniqueID`s referencing the `<LEDstate>`s that make up this state.
    pub led_state_refs: Vec<String>,
}

// --- Modular Device Management (New) ---

/// Represents the `<moduleManagement>` block from the *Device* profile.
#[derive(Debug, Default, PartialEq)]
pub struct ModuleManagementDevice {
    /// A list of interfaces (e.g., bus controllers) on the head module.
    pub interfaces: Vec<InterfaceDevice>,
    /// Information about this device, if it is *also* a module (child).
    pub module_interface: Option<ModuleInterface>,
}

/// Represents an `<interface>` on a modular head (Device profile).
#[derive(Debug, Default, PartialEq)]
pub struct InterfaceDevice {
    /// The unique ID for this interface, referenced by the Communication profile.
    pub unique_id: String,
    /// The type of interface (e.g., "X2X").
    pub interface_type: String,
    /// The maximum number of child modules this interface supports.
    pub max_modules: u32,
    /// Defines how child modules are addressed (`manual` or `position`).
    pub module_addressing: String, // Mapped from `ModuleAddressingHead`
    /// A list of XDC/XDD files for modules that can be connected.
    pub file_list: Vec<String>, // List of URIs
    /// A list of modules that are pre-configured in this XDC.
    pub connected_modules: Vec<ConnectedModule>,
}

/// Represents a `<connectedModule>` entry.
#[derive(Debug, Default, PartialEq)]
pub struct ConnectedModule {
    /// The `@childIDRef` linking to a `childID` from a module's XDC.
    pub child_id_ref: String,
    /// The physical position (slot) of the module, 1-based.
    pub position: u32,
    /// The bus address, if different from the position.
    pub address: Option<u32>,
}

/// Represents a `<moduleInterface>` (a child module's properties).
#[derive(Debug, Default, PartialEq)]
pub struct ModuleInterface {
    /// The unique ID of this child module.
    pub child_id: String,
    /// The type of interface this module connects to (e.g., "X2X").
    pub interface_type: String,
    /// The addressing mode this module supports (`manual`, `position`, `next`).
    pub module_addressing: String, // Mapped from `ModuleAddressingChild`
}

/// Represents the `<moduleManagement>` block from the *Communication* profile.
#[derive(Debug, Default, PartialEq)]
pub struct ModuleManagementComm {
    /// A list of interfaces and their OD range definitions.
    pub interfaces: Vec<InterfaceComm>,
}

/// Represents an `<interface>` in the Communication profile.
#[derive(Debug, Default, PartialEq)]
pub struct InterfaceComm {
    /// The `uniqueID` of the corresponding interface in the Device profile.
    pub unique_id_ref: String,
    /// The list of OD index ranges assigned to this interface.
    pub ranges: Vec<Range>,
}

/// Represents a `<range>` of OD indices for a modular interface.
#[derive(Debug, Default, PartialEq)]
pub struct Range {
    pub name: String,
    /// The starting index (e.g., 0x3000).
    pub base_index: u16,
    /// The maximum index (e.g., 0x3FFF).
    pub max_index: Option<u16>,
    /// The maximum sub-index (e.g., 0xFF).
    pub max_sub_index: u8,
    /// How to assign new objects (`index` or `subindex`).
    pub sort_mode: String, // Mapped from `SortMode`
    /// How to calculate the next index/sub-index (`continuous` or `address`).
    pub sort_number: String, // Mapped from `AddressingAttribute`
    /// The step size between new indices.
    pub sort_step: Option<u32>,
    /// The default PDO mapping for objects created in this range.
    pub pdo_mapping: Option<ObjectPdoMapping>,
}

// --- Network Management ---

/// Represents the `<NetworkManagement>` block from the Comm Profile.
#[derive(Debug, Default, PartialEq)]
pub struct NetworkManagement {
    pub general_features: GeneralFeatures,
    pub mn_features: Option<MnFeatures>,
    pub cn_features: Option<CnFeatures>,
    pub diagnostic: Option<Diagnostic>,
    // Add deviceCommissioning later if needed
}

/// Represents `<GeneralFeatures>`.
#[derive(Debug, Default, PartialEq)]
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
#[derive(Debug, Default, PartialEq)]
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
#[derive(Debug, Default, PartialEq)]
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
#[derive(Debug, Default, PartialEq)]
pub struct Diagnostic {
    /// All defined `<Error>` elements.
    pub errors: Vec<ErrorDefinition>,
    /// All defined `<ErrorBit>` elements from `<StaticErrorBitField>`.
    pub static_error_bit_field: Option<Vec<StaticErrorBit>>,
}

/// Represents one `<Error>` in the `<ErrorList>`.
#[derive(Debug, Default, PartialEq)]
pub struct ErrorDefinition {
    pub name: String,
    pub value: String, 
    pub add_info: Vec<AddInfo>,
}

/// Represents one `<addInfo>` element from an `<Error>`.
#[derive(Debug, Default, PartialEq)]
pub struct AddInfo {
    pub name: String,
    pub bit_offset: u8,
    pub len: u8,
    pub description: Option<String>,
}

/// Represents one `<ErrorBit>` from the `<StaticErrorBitField>`.
#[derive(Debug, Default, PartialEq)]
pub struct StaticErrorBit {
    pub name: String,
    pub offset: u8,
    pub label: Option<String>,
    pub description: Option<String>,
}

// --- Application Process ---

/// Represents the `<ApplicationProcess>` block, containing user-defined
/// data types, parameters, and groupings.
#[derive(Debug, Default, PartialEq)]
pub struct ApplicationProcess {
    /// List of user-defined data types.
    pub data_types: Vec<AppDataType>,
    /// List of parameter groups.
    pub parameter_groups: Vec<ParameterGroup>,
    /// List of function type definitions.
    pub function_types: Vec<FunctionType>,
    /// List of function instances.
    pub function_instances: Vec<FunctionInstance>,
}

/// An enum representing a user-defined data type from `<dataTypeList>`.
#[derive(Debug, PartialEq)]
pub enum AppDataType {
    Struct(AppStruct),
    Array(AppArray),
    Enum(AppEnum),
    Derived(AppDerived),
}

/// Represents a `<struct>` data type.
#[derive(Debug, Default, PartialEq)]
pub struct AppStruct {
    pub name: String,
    pub unique_id: String,
    pub label: Option<String>,
    pub description: Option<String>,
    pub members: Vec<StructMember>,
}

/// Represents a `<varDeclaration>` within a `<struct>`.
#[derive(Debug, Default, PartialEq)]
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
#[derive(Debug, Default, PartialEq)]
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
#[derive(Debug, Default, PartialEq)]
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
#[derive(Debug, Default, PartialEq)]
pub struct EnumValue {
    pub name: String,
    pub value: String,
    pub label: Option<String>,
    pub description: Option<String>,
}

/// Represents a `<derived>` data type.
#[derive(Debug, Default, PartialEq)]
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
#[derive(Debug, Default, PartialEq)]
pub struct Count {
    pub unique_id: String,
    pub access: Option<ParameterAccess>,
    pub default_value: Option<String>,
}

/// Represents a `<parameterGroup>` from the `<parameterGroupList>`.
#[derive(Debug, Default, PartialEq)]
pub struct ParameterGroup {
    pub unique_id: String,
    pub label: Option<String>,
    pub description: Option<String>,
    /// Nested groups or parameter references.
    pub items: Vec<ParameterGroupItem>,
}

/// An enum representing an item inside a `<parameterGroup>`.
#[derive(Debug, PartialEq)]
pub enum ParameterGroupItem {
    /// A nested parameter group.
    Group(ParameterGroup),
    /// A reference to a parameter.
    Parameter(ParameterRef),
}

/// Represents a `<parameterRef>` inside a `<parameterGroup>`.
#[derive(Debug, Default, PartialEq)]
pub struct ParameterRef {
    /// The `uniqueID` of the parameter being referenced.
    pub unique_id_ref: String,
    pub visible: bool,
    pub locked: bool,
    /// Optional bit offset for bit-packed groups.
    pub bit_offset: Option<u32>,
}

/// Represents a `<functionType>` (EPSG 311, 7.4.7.4).
#[derive(Debug, Default, PartialEq)]
pub struct FunctionType {
    pub name: String,
    pub unique_id: String,
    pub package: Option<String>,
    pub label: Option<String>,
    pub description: Option<String>,
    pub version_info: Vec<VersionInfo>,
    pub interface: InterfaceList,
}

/// Represents a `<versionInfo>` element (EPSG 311, 7.4.7.4.2).
#[derive(Debug, Default, PartialEq)]
pub struct VersionInfo {
    pub organization: String,
    pub version: String,
    pub author: String,
    pub date: String,
    pub label: Option<String>,
    pub description: Option<String>,
}

/// Represents an `<interfaceList>` for a function type (EPSG 311, 7.4.7.4.3).
#[derive(Debug, Default, PartialEq)]
pub struct InterfaceList {
    pub inputs: Vec<VarDeclaration>,
    pub outputs: Vec<VarDeclaration>,
    pub configs: Vec<VarDeclaration>,
}

/// Represents a `<varDeclaration>` within an `<interfaceList>`.
#[derive(Debug, Default, PartialEq)]
pub struct VarDeclaration {
    pub name: String,
    pub unique_id: String,
    pub data_type: String,
    pub size: Option<u32>,
    pub initial_value: Option<String>,
    pub label: Option<String>,
    pub description: Option<String>,
}

/// Represents a `<functionInstance>` (EPSG 311, 7.4.7.5.2).
#[derive(Debug, Default, PartialEq)]
pub struct FunctionInstance {
    pub name: String,
    pub unique_id: String,
    /// The `uniqueID` of the `<functionType>` this is an instance of.
    pub type_id_ref: String,
    pub label: Option<String>,
    pub description: Option<String>,
    // Connections are not resolved onto the instance, they are app-level
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
#[derive(Debug, Default, PartialEq)]
pub struct ObjectDictionary {
    pub objects: Vec<Object>,
}

/// Represents a single `<Object>` (an OD Index).
#[derive(Debug, Default, PartialEq)]
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
#[derive(Debug, Default, PartialEq)]
pub struct SubObject {
    /// `@subIndex` (as a u8, parsed from hex)
    pub sub_index: u8,
    
    // --- Metadata ---
    /// `@name`
    pub name: String,
    /// `@objectType` (e.Data, "7" for VAR)
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