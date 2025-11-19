//! Public, ergonomic data structures for representing a parsed XDC file.
//!
//! These types are the primary interface for consumers of this crate. They abstract
//! away the complexity of the underlying XML schema (handled by the `model` module)
//! and provide resolved, easy-to-use structures.
//!
//! **Note on Data Storage:**
//! Values (e.g., `Object::data`, `Parameter::actual_value`) are stored as `String`s
//! (e.g., "0x1234", "500"). This maintains fidelity to the XML source and allows
//! high-level manipulation without forcing immediate conversion to native binary types.
//! Conversion to `powerlink-rs` native types happens in the `converter` module.

use alloc::string::String;
use alloc::vec::Vec;

/// Represents a fully parsed and resolved XDC/XDD file.
///
/// This is the root structure returned by `load_xdc_from_str` or `load_xdd_defaults_from_str`.
#[derive(Debug, Default, PartialEq)]
pub struct XdcFile {
    /// Metadata from the `<ProfileHeader>` block.
    pub header: ProfileHeader,

    /// Information from the `<DeviceIdentity>` block.
    pub identity: Identity,

    /// Information from the `<DeviceFunction>` block (e.g., capabilities, connectors).
    pub device_function: Vec<DeviceFunction>,

    /// Information from the `<DeviceManager>` block (e.g., LEDs, modular management).
    pub device_manager: Option<DeviceManager>,

    /// Information from the `<NetworkManagement>` block (e.g., cycle timing, feature flags).
    pub network_management: Option<NetworkManagement>,

    /// Information from the `<ApplicationProcess>` block (e.g., parameters, templates).
    pub application_process: Option<ApplicationProcess>,

    /// The complete Object Dictionary for the device.
    pub object_dictionary: ObjectDictionary,

    /// Information from the `<moduleManagement>` block in the *Communication Profile*.
    pub module_management_comm: Option<ModuleManagementComm>,
}

/// Represents the `<ProfileHeader>` block, containing file metadata.
#[derive(Debug, Default, PartialEq)]
pub struct ProfileHeader {
    /// The profile identification string.
    pub identification: String,
    /// The profile revision.
    pub revision: String,
    /// The profile name.
    pub name: String,
    /// The source/creator of the profile.
    pub source: String,
    /// The profile creation date (ISO 8601).
    pub date: Option<String>,
}

/// Represents the `<DeviceIdentity>` block.
#[derive(Debug, Default, PartialEq)]
pub struct Identity {
    /// The vendor name.
    pub vendor_name: String,
    /// The unique vendor ID (parsed from hex to u32).
    pub vendor_id: u32,
    /// Descriptive text about the vendor (first available label).
    pub vendor_text: Option<String>,

    /// The device family name.
    pub device_family: Option<String>,
    /// The product family name.
    pub product_family: Option<String>,

    /// The product name.
    pub product_name: String,
    /// The unique product ID (parsed from hex to u32).
    pub product_id: u32,
    /// Descriptive text about the product.
    pub product_text: Option<String>,

    /// List of order numbers associated with the device.
    pub order_number: Vec<String>,
    /// List of version entries (HW, SW, FW).
    pub versions: Vec<Version>,

    /// The build date of the device definition.
    pub build_date: Option<String>,
    /// The revision of the specification used.
    pub specification_revision: Option<String>,
    /// The specific instance name of the device.
    pub instance_name: Option<String>,
}

/// Represents a `<version>` element.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Version {
    /// The type of version (e.g., "HW", "SW", "FW").
    pub version_type: String,
    /// The version value.
    pub value: String,
}

/// Represents the `<DeviceFunction>` block.
/// (EPSG DS 311, 7.4.6)
#[derive(Debug, Default, PartialEq)]
pub struct DeviceFunction {
    /// Device capabilities and standard compliance information.
    pub capabilities: Option<Capabilities>,
    /// Links to device pictures or icons.
    pub pictures: Vec<Picture>,
    /// Links to external text resource files.
    pub dictionaries: Vec<Dictionary>,
    /// Definitions of physical connectors.
    pub connectors: Vec<Connector>,
    /// Links to firmware files.
    pub firmware_list: Vec<Firmware>,
    /// List of classification keywords (e.g., "IO", "Drive").
    pub classifications: Vec<Classification>,
}

/// Represents the `<capabilities>` element.
/// (EPSG DS 311, 7.4.6.2)
#[derive(Debug, Default, PartialEq)]
pub struct Capabilities {
    /// A list of characteristics, often grouped by category.
    pub characteristics: Vec<CharacteristicList>,
    /// A list of standards this device complies with.
    pub standard_compliance: Vec<StandardCompliance>,
}

/// Represents a `<characteristicsList>`, grouping characteristics by category.
/// (EPSG DS 311, 7.4.6.2.2)
#[derive(Debug, Default, PartialEq)]
pub struct CharacteristicList {
    /// An optional category name for this group.
    pub category: Option<String>,
    /// The list of characteristics in this group.
    pub characteristics: Vec<Characteristic>,
}

/// Represents a single `<characteristic>`.
/// (EPSG DS 311, 7.4.6.2.2.2)
#[derive(Debug, Default, PartialEq)]
pub struct Characteristic {
    /// The name of the characteristic (e.g., "Transfer rate").
    pub name: String,
    /// A list of values for this characteristic (e.g., "100 MBit/s").
    pub content: Vec<String>,
}

/// Represents a `<compliantWith>` element describing standard compliance.
/// (EPSG DS 311, 7.4.6.2.2.5)
#[derive(Debug, Default, PartialEq)]
pub struct StandardCompliance {
    /// The name of the standard (e.g., "EN 61131-2").
    pub name: String,
    /// The range of compliance, either "international" or "internal".
    pub range: String,
    /// An optional description.
    pub description: Option<String>,
}

/// Represents a `<picture>` element.
/// (EPSG DS 311, 7.4.6.3)
#[derive(Debug, Default, PartialEq)]
pub struct Picture {
    /// The URI to the picture file.
    pub uri: String,
    /// The type of picture ("frontPicture", "icon", "additional", "none").
    pub picture_type: String,
    /// An optional number/index for the picture.
    pub number: Option<u32>,
    /// An optional label.
    pub label: Option<String>,
    /// An optional description.
    pub description: Option<String>,
}

/// Represents a `<dictionary>` element for external text resources.
/// (EPSG DS 311, 7.4.6.4)
#[derive(Debug, Default, PartialEq)]
pub struct Dictionary {
    /// The URI to the text resource file.
    pub uri: String,
    /// The language of the dictionary (e.g., "en", "de").
    pub lang: String,
    /// The ID used to reference this dictionary within the XDC.
    pub dict_id: String,
}

/// Represents a `<connector>` element.
/// (EPSG DS 311, 7.4.6.5)
#[derive(Debug, Default, PartialEq)]
pub struct Connector {
    /// The ID of the connector.
    pub id: String,
    /// The type of connector (e.g., "POWERLINK", "RJ45").
    pub connector_type: String,
    /// Optional reference to a modular interface ID.
    pub interface_id_ref: Option<String>,
    /// Optional label.
    pub label: Option<String>,
    /// Optional description.
    pub description: Option<String>,
}

/// Represents a `<firmware>` element.
/// (EPSG DS 311, 7.4.6.6)
#[derive(Debug, Default, PartialEq)]
pub struct Firmware {
    /// The URI to the firmware file.
    pub uri: String,
    /// The revision number this firmware corresponds to.
    pub device_revision_number: u32,
    /// Optional build date of the firmware.
    pub build_date: Option<String>,
    /// Optional label.
    pub label: Option<String>,
    /// Optional description.
    pub description: Option<String>,
}

/// Represents a `<classification>` element.
/// (EPSG DS 311, 7.4.6.7)
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

/// Represents an `<indicatorList>` containing LED definitions.
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
    /// Color configuration ("monocolor" or "bicolor").
    pub colors: String,
    /// The functionality type ("IO", "device", "communication").
    pub led_type: Option<String>,
    /// A list of all defined states for this LED.
    pub states: Vec<LEDstate>,
}

/// Represents a single state for a specific `<LED>` (e.g., "flashing red").
#[derive(Debug, Default, PartialEq)]
pub struct LEDstate {
    /// The unique ID used to reference this state.
    pub unique_id: String,
    /// The state ("on", "off", "flashing").
    pub state: String,
    /// The color in this state ("green", "amber", "red").
    pub color: String,
    /// Primary label for this state.
    pub label: Option<String>,
    /// Description of this state.
    pub description: Option<String>,
}

/// Represents a state composed of multiple LEDs (e.g., "Error Stop").
#[derive(Debug, Default, PartialEq)]
pub struct CombinedState {
    /// Primary label for this combined state.
    pub label: Option<String>,
    /// Description of this combined state.
    pub description: Option<String>,
    /// A list of `uniqueID`s referencing the constituent `<LEDstate>`s.
    pub led_state_refs: Vec<String>,
}

// --- Modular Device Management ---

/// Represents the `<moduleManagement>` block from the *Device* profile.
#[derive(Debug, Default, PartialEq)]
pub struct ModuleManagementDevice {
    /// A list of interfaces (e.g., bus controllers) on the head module.
    pub interfaces: Vec<InterfaceDevice>,
    /// Information about this device if it acts as a module (child).
    pub module_interface: Option<ModuleInterface>,
}

/// Represents an `<interface>` on a modular head (Device profile).
#[derive(Debug, Default, PartialEq)]
pub struct InterfaceDevice {
    /// The unique ID for this interface.
    pub unique_id: String,
    /// The type of interface (e.g., "X2X").
    pub interface_type: String,
    /// The maximum number of child modules supported.
    pub max_modules: u32,
    /// Addressing mode for child modules (`manual` or `position`).
    pub module_addressing: String,
    /// A list of URIs to XDC/XDD files for compatible modules.
    pub file_list: Vec<String>,
    /// A list of pre-configured/connected modules.
    pub connected_modules: Vec<ConnectedModule>,
}

/// Represents a `<connectedModule>` entry, linking a slot to a child module.
#[derive(Debug, Default, PartialEq)]
pub struct ConnectedModule {
    /// The reference to a `childID` in a module's XDC.
    pub child_id_ref: String,
    /// The physical position (slot), 1-based.
    pub position: u32,
    /// The bus address, if different from position.
    pub address: Option<u32>,
}

/// Represents a `<moduleInterface>` (properties of a child module).
#[derive(Debug, Default, PartialEq)]
pub struct ModuleInterface {
    /// The unique ID of this child module.
    pub child_id: String,
    /// The interface type this module connects to.
    pub interface_type: String,
    /// Supported addressing mode (`manual`, `position`, `next`).
    pub module_addressing: String,
}

/// Represents the `<moduleManagement>` block from the *Communication* profile.
#[derive(Debug, Default, PartialEq)]
pub struct ModuleManagementComm {
    /// A list of interfaces and their Object Dictionary range definitions.
    pub interfaces: Vec<InterfaceComm>,
}

/// Represents an `<interface>` in the Communication profile, mapping hardware interfaces to OD ranges.
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
    /// Name of the range.
    pub name: String,
    /// The starting index (e.g., 0x3000).
    pub base_index: u16,
    /// The maximum index (e.g., 0x3FFF).
    pub max_index: Option<u16>,
    /// The maximum sub-index (e.g., 0xFF).
    pub max_sub_index: u8,
    /// Assignment mode (`index` or `subindex`).
    pub sort_mode: String,
    /// Calculation mode for next index (`continuous` or `address`).
    pub sort_number: String,
    /// The step size between new indices.
    pub sort_step: Option<u32>,
    /// The default PDO mapping for objects created in this range.
    pub pdo_mapping: Option<ObjectPdoMapping>,
}

// --- Network Management ---

/// Represents the `<NetworkManagement>` block.
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
    /// `DLLFeatureMN`: Supports Managing Node functionality.
    pub dll_feature_mn: bool,
    /// `NMTBootTimeNotActive`: Time in microseconds to wait in NmtNotActive.
    pub nmt_boot_time_not_active: u32,
    /// `NMTCycleTimeMax`: Max cycle time in microseconds.
    pub nmt_cycle_time_max: u32,
    /// `NMTCycleTimeMin`: Min cycle time in microseconds.
    pub nmt_cycle_time_min: u32,
    /// `NMTErrorEntries`: Size of error history.
    pub nmt_error_entries: u32,
    /// `NMTMaxCNNumber`: Max number of CNs.
    pub nmt_max_cn_number: Option<u8>,
    /// `PDODynamicMapping`: Supports dynamic PDO mapping.
    pub pdo_dynamic_mapping: Option<bool>,
    /// `SDOClient`: Supports SDO Client.
    pub sdo_client: Option<bool>,
    /// `SDOServer`: Supports SDO Server.
    pub sdo_server: Option<bool>,
    /// `SDOSupportASnd`: Supports SDO over ASnd.
    pub sdo_support_asnd: Option<bool>,
    /// `SDOSupportUdpIp`: Supports SDO over UDP/IP.
    pub sdo_support_udp_ip: Option<bool>,

    /// `NMTIsochronous`: Supports isochronous operation.
    pub nmt_isochronous: Option<bool>,
    /// `SDOSupportPDO`: Supports SDO embedded in PDO.
    pub sdo_support_pdo: Option<bool>,
    /// `NMTExtNmtCmds`: Supports extended NMT commands.
    pub nmt_ext_nmt_cmds: Option<bool>,
    /// `CFMConfigManager`: Supports Configuration Manager.
    pub cfm_config_manager: Option<bool>,
    /// `NMTNodeIDBySW`: Supports setting Node ID via software.
    pub nmt_node_id_by_sw: Option<bool>,
    /// `SDOCmdReadAllByIndex`: Supports reading all sub-indices.
    pub sdo_cmd_read_all_by_index: Option<bool>,
    /// `SDOCmdWriteAllByIndex`: Supports writing all sub-indices.
    pub sdo_cmd_write_all_by_index: Option<bool>,
    /// `SDOCmdReadMultParam`: Supports multiple parameter read.
    pub sdo_cmd_read_mult_param: Option<bool>,
    /// `SDOCmdWriteMultParam`: Supports multiple parameter write.
    pub sdo_cmd_write_mult_param: Option<bool>,
    /// `NMTPublishActiveNodes`: Supports publishing Active Nodes list.
    pub nmt_publish_active_nodes: Option<bool>,
    /// `NMTPublishConfigNodes`: Supports publishing Configured Nodes list.
    pub nmt_publish_config_nodes: Option<bool>,
}

/// Represents `<MNFeatures>`, specific to Managing Nodes.
#[derive(Debug, Default, PartialEq)]
pub struct MnFeatures {
    /// `DLLMNFeatureMultiplex`: Supports multiplexing.
    pub dll_mn_feature_multiplex: Option<bool>,
    /// `DLLMNPResChaining`: Supports PRes Chaining.
    pub dll_mn_pres_chaining: Option<bool>,
    /// `NMTSimpleBoot`: Supports simple boot-up.
    pub nmt_simple_boot: bool,

    /// `NMTServiceUdpIp`: Supports NMT services over UDP.
    pub nmt_service_udp_ip: Option<bool>,
    /// `NMTMNBasicEthernet`: Supports Basic Ethernet mode.
    pub nmt_mn_basic_ethernet: Option<bool>,
}

/// Public representation of the `NMTCNDNA` attribute (Dynamic Node Addressing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NmtCnDna {
    /// Do not clear configuration.
    DoNotClear,
    /// Clear configuration on transition PRE_OP1 -> PRE_OP2.
    ClearOnPreOp1ToPreOp2,
    /// Clear configuration on NMT_Reset_Node.
    ClearOnNmtResetNode,
}

/// Represents `<CNFeatures>`, specific to Controlled Nodes.
#[derive(Debug, Default, PartialEq)]
pub struct CnFeatures {
    /// `DLLCNFeatureMultiplex`: Supports multiplexing.
    pub dll_cn_feature_multiplex: Option<bool>,
    /// `DLLCNPResChaining`: Supports PRes Chaining.
    pub dll_cn_pres_chaining: Option<bool>,
    /// `NMTCNPreOp2ToReady2Op`: Transition time in nanoseconds.
    pub nmt_cn_pre_op2_to_ready2_op: Option<u32>,
    /// `NMTCNSoC2PReq`: SoC to PReq latency in nanoseconds.
    pub nmt_cn_soc_2_preq: u32,
    /// `NMTCNDNA`: Dynamic Node Addressing behavior.
    pub nmt_cn_dna: Option<NmtCnDna>,
}

/// Represents `<Diagnostic>` capabilities.
#[derive(Debug, Default, PartialEq)]
pub struct Diagnostic {
    /// List of defined errors.
    pub errors: Vec<ErrorDefinition>,
    /// Definitions for bits in the Static Error Bit Field.
    pub static_error_bit_field: Option<Vec<StaticErrorBit>>,
}

/// Represents one `<Error>` entry in the `<ErrorList>`.
#[derive(Debug, Default, PartialEq)]
pub struct ErrorDefinition {
    /// The name of the error.
    pub name: String,
    /// The error code value.
    pub value: String,
    /// Additional information fields.
    pub add_info: Vec<AddInfo>,
}

/// Represents one `<addInfo>` element.
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

/// Represents `<allowedValues>` for a parameter.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AllowedValues {
    /// Optional reference to a template.
    pub template_id_ref: Option<String>,
    /// List of enumerated allowed values.
    pub values: Vec<Value>,
    /// List of allowed ranges.
    pub ranges: Vec<ValueRange>,
}

/// Represents a single `<value>`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Value {
    /// The literal value string (e.g., "1", "0x0A").
    pub value: String,
    /// Optional label.
    pub label: Option<String>,
    /// Optional offset for scaling.
    pub offset: Option<String>,
    /// Optional multiplier for scaling.
    pub multiplier: Option<String>,
}

/// Represents a `<range>`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ValueRange {
    /// The minimum value.
    pub min_value: String,
    /// The maximum value.
    pub max_value: String,
    /// Optional step size.
    pub step: Option<String>,
}

/// Represents the `<ApplicationProcess>` block, defining application parameters and types.
#[derive(Debug, Default, PartialEq)]
pub struct ApplicationProcess {
    /// User-defined data types.
    pub data_types: Vec<AppDataType>,
    /// Parameter templates.
    pub templates: Vec<Parameter>,
    /// Actual parameters.
    pub parameters: Vec<Parameter>,
    /// Parameter groupings.
    pub parameter_groups: Vec<ParameterGroup>,
    /// Function type definitions.
    pub function_types: Vec<FunctionType>,
    /// Function instances.
    pub function_instances: Vec<FunctionInstance>,
}

/// Enum representing user-defined data types from `<dataTypeList>`.
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

/// Represents a `<varDeclaration>` within a struct.
#[derive(Debug, Default, PartialEq)]
pub struct StructMember {
    pub name: String,
    pub unique_id: String,
    /// The data type ID or name.
    pub data_type: String,
    /// Size in bits.
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
    /// The data type of array elements.
    pub data_type: String,
}

/// Represents an `<enum>` data type.
#[derive(Debug, Default, PartialEq)]
pub struct AppEnum {
    pub name: String,
    pub unique_id: String,
    pub label: Option<String>,
    pub description: Option<String>,
    /// The base data type.
    pub data_type: String,
    pub size_in_bits: Option<u32>,
    pub values: Vec<EnumValue>,
}

/// Represents a single `<enumValue>`.
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
    /// The base data type.
    pub data_type: String,
    pub count: Option<Count>,
}

/// Represents a `<count>` element within a derived type.
#[derive(Debug, Default, PartialEq)]
pub struct Count {
    pub unique_id: String,
    pub access: Option<ParameterAccess>,
    pub default_value: Option<String>,
}

/// Represents a `<parameterGroup>`.
#[derive(Debug, Default, PartialEq)]
pub struct ParameterGroup {
    pub unique_id: String,
    pub label: Option<String>,
    pub description: Option<String>,
    /// Nested groups or parameter references.
    pub items: Vec<ParameterGroupItem>,
}

/// An item inside a `<parameterGroup>`.
#[derive(Debug, PartialEq)]
pub enum ParameterGroupItem {
    Group(ParameterGroup),
    Parameter(ParameterRef),
}

/// Represents a reference to a parameter within a group.
#[derive(Debug, Default, PartialEq)]
pub struct ParameterRef {
    pub unique_id_ref: String,
    pub visible: bool,
    pub locked: bool,
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
    pub unique_id: String,
    pub access: Option<ParameterAccess>,
    pub support: Option<ParameterSupport>,
    pub persistent: bool,
    pub offset: Option<String>,
    pub multiplier: Option<String>,
    pub template_id_ref: Option<String>,
    pub data_type: ParameterDataType,
    pub label: Option<String>,
    pub description: Option<String>,
    pub actual_value: Option<Value>,
    pub default_value: Option<Value>,
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

/// Represents an `<interfaceList>`.
#[derive(Debug, Default, PartialEq)]
pub struct InterfaceList {
    pub inputs: Vec<VarDeclaration>,
    pub outputs: Vec<VarDeclaration>,
    pub configs: Vec<VarDeclaration>,
}

/// Represents a `<varDeclaration>` in an interface list.
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
    /// The object index (parsed from hex string).
    pub index: u16,

    // --- Metadata ---
    /// Object name.
    pub name: String,
    /// Object type (e.g., "7" for VAR, "8" for ARRAY, "9" for RECORD).
    pub object_type: String,
    /// Data type ID (e.g., "0006").
    pub data_type: Option<String>,
    /// Low limit for the value.
    pub low_limit: Option<String>,
    /// High limit for the value.
    pub high_limit: Option<String>,
    /// Access type.
    pub access_type: Option<ParameterAccess>,
    /// PDO mapping capability.
    pub pdo_mapping: Option<ObjectPdoMapping>,
    /// Object flags.
    pub obj_flags: Option<String>,
    /// Support level.
    pub support: Option<ParameterSupport>,
    /// Persistence flag.
    pub persistent: bool,
    /// Allowed values constraint.
    pub allowed_values: Option<AllowedValues>,

    // --- Value ---
    /// The resolved value for this object.
    ///
    /// This prioritizes `actualValue` (XDC) or `defaultValue` (XDD) depending on the
    /// parsing mode. It resolves `uniqueIDRef` links to Application Process parameters.
    /// stored as a human-readable string.
    pub data: Option<String>,

    // --- Children ---
    /// List of sub-objects.
    pub sub_objects: Vec<SubObject>,
}

/// Represents a `<SubObject>` (an OD Sub-Index).
#[derive(Debug, Default, PartialEq)]
pub struct SubObject {
    /// The sub-index (parsed from hex string).
    pub sub_index: u8,

    // --- Metadata ---
    pub name: String,
    pub object_type: String,
    pub data_type: Option<String>,
    pub low_limit: Option<String>,
    pub high_limit: Option<String>,
    pub access_type: Option<ParameterAccess>,
    pub pdo_mapping: Option<ObjectPdoMapping>,
    pub obj_flags: Option<String>,
    pub support: Option<ParameterSupport>,
    pub persistent: bool,
    pub allowed_values: Option<AllowedValues>,

    // --- Value ---
    /// The resolved value for this sub-object (human-readable string).
    pub data: Option<String>,
}
