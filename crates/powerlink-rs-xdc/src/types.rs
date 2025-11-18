// crates/powerlink-rs-xdc/src/types.rs

//! Public, ergonomic data structures for representing a parsed XDC file.

use alloc::string::String;
use alloc::vec::Vec;

// --- Root XDC Structure ---

/// Represents a fully parsed and resolved XDC/XDD file.
#[derive(Debug, Default, PartialEq)]
pub struct XdcFile {
    /// Metadata from the `<ProfileHeader>` block.
    pub header: ProfileHeader,

    /// Information from the `<DeviceIdentity>` block.
    pub identity: Identity,

    /// Information from the `<DeviceFunction>` block.
    pub device_function: Vec<DeviceFunction>,

    /// Information from the `<DeviceManager>` block.
    pub device_manager: Option<DeviceManager>,

    /// Information from the `<NetworkManagement>` block.
    pub network_management: Option<NetworkManagement>,

    /// Information from the `<ApplicationProcess>` block.
    pub application_process: Option<ApplicationProcess>,

    /// The complete Object Dictionary for the device.
    pub object_dictionary: ObjectDictionary,

    /// Information from the `<moduleManagement>` block in the *Communication Profile*.
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
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Version {
    /// `@versionType`
    pub version_type: String,
    /// `@value`
    pub value: String,
}

// --- Device Function ---

/// Represents the `<DeviceFunction>` block (EPSG DS 311, 7.4.6).
#[derive(Debug, Default, PartialEq)]
pub struct DeviceFunction {
    /// Contains device capabilities and standard compliance.
    pub capabilities: Option<Capabilities>,
    /// Contains links to device pictures or icons.
    pub pictures: Vec<Picture>,
    /// Contains links to external text resource files.
    pub dictionaries: Vec<Dictionary>,
    /// Contains definitions of physical connectors.
    pub connectors: Vec<Connector>,
    /// Contains links to firmware files.
    pub firmware_list: Vec<Firmware>,
    /// Contains a list of classification keywords.
    pub classifications: Vec<Classification>,
}

/// Represents the `<capabilities>` element (EPSG DS 311, 7.4.6.2).
#[derive(Debug, Default, PartialEq)]
pub struct Capabilities {
    /// A list of characteristics, often grouped by category.
    pub characteristics: Vec<CharacteristicList>,
    /// A list of standards this device complies with.
    pub standard_compliance: Vec<StandardCompliance>,
}

/// Represents a `<characteristicsList>` (EPSG DS 311, 7.4.6.2.2).
#[derive(Debug, Default, PartialEq)]
pub struct CharacteristicList {
    /// An optional category name for this group of characteristics.
    pub category: Option<String>,
    /// The list of characteristics in this group.
    pub characteristics: Vec<Characteristic>,
}

/// Represents a single `<characteristic>` (EPSG DS 311, 7.4.6.2.2.2).
#[derive(Debug, Default, PartialEq)]
pub struct Characteristic {
    /// The name of the characteristic (e.g., "Transfer rate").
    pub name: String,
    /// A list of values for this characteristic (e.g., "100 MBit/s").
    pub content: Vec<String>,
}

/// Represents a `<compliantWith>` element (EPSG DS 311, 7.4.6.2.2.5).
#[derive(Debug, Default, PartialEq)]
pub struct StandardCompliance {
    /// The name of the standard (e.g., "EN 61131-2").
    pub name: String,
    /// The range, either "international" or "internal".
    pub range: String,
    /// An optional description (from `<label>`).
    pub description: Option<String>,
}

/// Represents a `<picture>` element (EPSG DS 311, 7.4.6.3).
#[derive(Debug, Default, PartialEq)]
pub struct Picture {
    /// The link to the picture file.
    pub uri: String,
    /// The type of picture ("frontPicture", "icon", "additional", "none").
    pub picture_type: String,
    /// An optional number for the picture.
    pub number: Option<u32>,
    /// An optional label for the picture.
    pub label: Option<String>,
    /// An optional description for the picture.
    pub description: Option<String>,
}

/// Represents a `<dictionary>` element (EPSG DS 311, 7.4.6.4).
#[derive(Debug, Default, PartialEq)]
pub struct Dictionary {
    /// The link to the text resource file.
    pub uri: String,
    /// The language of the dictionary (e.g., "en", "de").
    pub lang: String,
    /// The ID used to reference this dictionary.
    pub dict_id: String,
}

/// Represents a `<connector>` element (EPSG DS 311, 7.4.6.5).
#[derive(Debug, Default, PartialEq)]
pub struct Connector {
    /// The ID of the connector.
    pub id: String,
    /// The type of connector (e.g., "POWERLINK", "RJ45").
    pub connector_type: String,
    /// Optional reference to a modular interface.
    pub interface_id_ref: Option<String>,
    /// Optional label for the connector.
    pub label: Option<String>,
    /// Optional description for the connector.
    pub description: Option<String>,
}

/// Represents a `<firmware>` element (EPSG DS 311, 7.4.6.6).
#[derive(Debug, Default, PartialEq)]
pub struct Firmware {
    /// The link to the firmware file.
    pub uri: String,
    /// The revision number this firmware corresponds to.
    pub device_revision_number: u32,
    /// Optional build date of the firmware.
    pub build_date: Option<String>,
    /// Optional label for the firmware.
    pub label: Option<String>,
    /// Optional description for the firmware.
    pub description: Option<String>,
}

/// Represents a `<classification>` element (EPSG DS 311, 7.4.6.7).
#[derive(Debug, Default, PartialEq)]
pub struct Classification {
    /// The classification value (e.g., "Controller", "IO", "Drive").
    pub value: String,
}

// --- Device Manager ---

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

// --- Modular Device Management ---

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

    /// `@NMTIsochronous`
    pub nmt_isochronous: Option<bool>,
    /// `@SDOSupportPDO`
    pub sdo_support_pdo: Option<bool>,
    /// `@NMTExtNmtCmds`
    pub nmt_ext_nmt_cmds: Option<bool>,
    /// `@CFMConfigManager`
    pub cfm_config_manager: Option<bool>,
    /// `@NMTNodeIDBySW`
    pub nmt_node_id_by_sw: Option<bool>,
    /// `@SDOCmdReadAllByIndex`
    pub sdo_cmd_read_all_by_index: Option<bool>,
    /// `@SDOCmdWriteAllByIndex`
    pub sdo_cmd_write_all_by_index: Option<bool>,
    /// `@SDOCmdReadMultParam`
    pub sdo_cmd_read_mult_param: Option<bool>,
    /// `@SDOCmdWriteMultParam`
    pub sdo_cmd_write_mult_param: Option<bool>,
    /// `@NMTPublishActiveNodes`
    pub nmt_publish_active_nodes: Option<bool>,
    /// `@NMTPublishConfigNodes`
    pub nmt_publish_config_nodes: Option<bool>,
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

    /// `@NMTServiceUdpIp`
    pub nmt_service_udp_ip: Option<bool>,
    /// `@NMTMNBasicEthernet`
    pub nmt_mn_basic_ethernet: Option<bool>,
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
    pub nmt_cn_dna: Option<NmtCnDna>,
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

/// Represents `<allowedValues>` from `<parameter>`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AllowedValues {
    /// An optional reference to a template allowedValues.
    pub template_id_ref: Option<String>,
    /// A list of enumerated allowed values.
    pub values: Vec<Value>,
    /// A list of allowed ranges.
    pub ranges: Vec<ValueRange>,
}

/// Represents a single `<value>` from `<allowedValues>`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Value {
    /// The literal value string (e.g., "1", "0x0A").
    pub value: String,
    /// An optional label for this value.
    pub label: Option<String>,
    /// Optional offset for scaling.
    pub offset: Option<String>,
    /// Optional multiplier for scaling.
    pub multiplier: Option<String>,
}

/// Represents a `<range>` from `<allowedValues>`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ValueRange {
    /// The literal `minValue` string.
    pub min_value: String,
    /// The literal `maxValue` string.
    pub max_value: String,
    /// An optional `step` string.
    pub step: Option<String>,
}

/// Represents the `<ApplicationProcess>` block.
#[derive(Debug, Default, PartialEq)]
pub struct ApplicationProcess {
    /// List of user-defined data types.
    pub data_types: Vec<AppDataType>,
    /// List of parameter templates.
    pub templates: Vec<Parameter>,
    /// List of actual parameters.
    pub parameters: Vec<Parameter>,
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
    /// The data type of this member.
    pub data_type: String,
    /// Size in bits, if applicable.
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
    /// The data type of the array elements.
    pub data_type: String,
}

/// Represents an `<enum>` data type.
#[derive(Debug, Default, PartialEq)]
pub struct AppEnum {
    pub name: String,
    pub unique_id: String,
    pub label: Option<String>,
    pub description: Option<String>,
    /// The base data type for the enum.
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
    /// The base data type this is derived from.
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

// --- Parameter ---

/// The data type of a parameter.
#[derive(Debug, Clone, PartialEq)]
pub enum ParameterDataType {
    // Simple types
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
    // Reference types
    DataTypeIDRef(String),
    VariableRef,
}

impl Default for ParameterDataType {
    fn default() -> Self {
        ParameterDataType::BOOL
    }
}

/// Represents a `<parameter>` or `<parameterTemplate>`.
#[derive(Debug, Default, PartialEq)]
pub struct Parameter {
    /// `@uniqueID`
    pub unique_id: String,
    /// `@access`
    pub access: Option<ParameterAccess>,
    /// `@support`
    pub support: Option<ParameterSupport>,
    /// `@persistent`
    pub persistent: bool,
    /// `@offset`
    pub offset: Option<String>,
    /// `@multiplier`
    pub multiplier: Option<String>,
    /// `@templateIDRef`
    pub template_id_ref: Option<String>,

    /// The data type of the parameter.
    pub data_type: ParameterDataType,

    /// Descriptive label.
    pub label: Option<String>,
    /// Descriptive text.
    pub description: Option<String>,

    /// `<actualValue>`
    pub actual_value: Option<Value>,
    /// `<defaultValue>`
    pub default_value: Option<Value>,
    /// `<allowedValues>`
    pub allowed_values: Option<AllowedValues>,
}

/// Represents a `<functionType>`.
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

/// Represents a `<versionInfo>` element.
#[derive(Debug, Default, PartialEq)]
pub struct VersionInfo {
    pub organization: String,
    pub version: String,
    pub author: String,
    pub date: String,
    pub label: Option<String>,
    pub description: Option<String>,
}

/// Represents an `<interfaceList>` for a function type.
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

/// Represents a `<functionInstance>`.
#[derive(Debug, Default, PartialEq)]
pub struct FunctionInstance {
    pub name: String,
    pub unique_id: String,
    /// The `uniqueID` of the `<functionType>` this is an instance of.
    pub type_id_ref: String,
    pub label: Option<String>,
    pub description: Option<String>,
}

// --- Object Dictionary ---

/// Access types for an Object Dictionary entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParameterAccess {
    Constant,
    ReadOnly,
    WriteOnly,
    ReadWrite,
    ReadWriteInput,
    ReadWriteOutput,
    NoAccess,
}

/// Support level for an Object Dictionary entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParameterSupport {
    Mandatory,
    Optional,
    Conditional,
}

/// PDO mapping capabilities.
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
    /// `@objectType`
    pub object_type: String,
    /// `@dataType`
    pub data_type: Option<String>,
    /// `@lowLimit`
    pub low_limit: Option<String>,
    /// `@highLimit`
    pub high_limit: Option<String>,
    /// Resolved access type.
    pub access_type: Option<ParameterAccess>,
    /// `@PDOmapping`
    pub pdo_mapping: Option<ObjectPdoMapping>,
    /// `@objFlags`
    pub obj_flags: Option<String>,
    /// Resolved support level.
    pub support: Option<ParameterSupport>,
    /// Resolved `persistent` flag.
    pub persistent: bool,
    /// Resolved `<allowedValues>`.
    pub allowed_values: Option<AllowedValues>,

    // --- Value ---
    /// The resolved data for this object (human-readable string).
    /// For RECORD types, this is None.
    pub data: Option<String>,

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
    /// `@objectType`
    pub object_type: String,
    /// `@dataType`
    pub data_type: Option<String>,
    /// `@lowLimit`
    pub low_limit: Option<String>,
    /// `@highLimit`
    pub high_limit: Option<String>,
    /// Resolved access type.
    pub access_type: Option<ParameterAccess>,
    /// `@PDOmapping`
    pub pdo_mapping: Option<ObjectPdoMapping>,
    /// `@objFlags`
    pub obj_flags: Option<String>,
    /// Resolved support level.
    pub support: Option<ParameterSupport>,
    /// Resolved `persistent` flag.
    pub persistent: bool,
    /// Resolved `<allowedValues>`.
    pub allowed_values: Option<AllowedValues>,

    // --- Value ---
    /// The resolved data for this sub-object (human-readable string).
    pub data: Option<String>,
}
