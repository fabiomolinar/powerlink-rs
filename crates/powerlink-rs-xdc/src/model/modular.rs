// crates/powerlink-rs-xdc/src/model/modular.rs

//! Contains model structs related to Modular Device Profiles.
//! (Schemas: `CommonElements_Modular.xsd`, `ProfileBody_Device_Powerlink_Modular_Head.xsd`,
//! `ProfileBody_Device_Powerlink_Modular_Child.xsd`,
//! `ProfileBody_CommunicationNetwork_Powerlink_Modular_Head.xsd`,
//! `ProfileBody_CommunicationNetwork_Powerlink_Modular_Child.xsd`)

use serde::{Deserialize, Serialize};
use alloc::vec::Vec;
use alloc::string::String;
use super::common::Glabels;
use super::app_layers::ObjectPdoMapping;

// --- Common Modular Types (from CommonElements_Modular.xsd) ---

/// Represents a `<file>` element.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct File {
    #[serde(rename = "@URI")]
    pub uri: String, // xsd:anyURI
}

/// Represents a `<fileList>` element.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct FileList {
    #[serde(rename = "file", default, skip_serializing_if = "Vec::is_empty")]
    pub file: Vec<File>,
}

/// Represents a `<moduleType>` element.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ModuleType {
    #[serde(rename = "@uniqueID")]
    pub unique_id: String, // xsd:ID
    #[serde(rename = "@type")]
    pub module_type: String, // xsd:NCName
}

/// Represents a `<moduleTypeList>` element.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ModuleTypeList {
    #[serde(rename = "moduleType", default, skip_serializing_if = "Vec::is_empty")]
    pub module_type: Vec<ModuleType>,
}

/// Represents the `@sortMode` attribute enum.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum SortMode {
    #[serde(rename = "index")]
    Index,
    #[serde(rename = "subindex")]
    Subindex,
}

// Fix: Add Default implementation
impl Default for SortMode {
    fn default() -> Self {
        Self::Index
    }
}

/// Represents the `@sortNumber` / `@addressingAttribute` attribute enum.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum AddressingAttribute {
    #[serde(rename = "continuous")]
    Continuous,
    #[serde(rename = "address")]
    Address,
}

// Fix: Add Default implementation
impl Default for AddressingAttribute {
    fn default() -> Self {
        Self::Continuous
    }
}

/// Represents the `@moduleAddressing` attribute enum for a modular child.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum ModuleAddressingChild {
    #[serde(rename = "manual")]
    Manual,
    #[serde(rename = "position")]
    Position,
    #[serde(rename = "next")]
    Next,
}

// Fix: Add Default implementation
impl Default for ModuleAddressingChild {
    fn default() -> Self {
        Self::Position
    }
}

/// Represents the `@moduleAddressing` attribute enum for a modular head.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum ModuleAddressingHead {
    #[serde(rename = "manual")]
    Manual,
    #[serde(rename = "position")]
    Position,
}

// Fix: Add Default implementation
impl Default for ModuleAddressingHead {
    fn default() -> Self {
        Self::Position
    }
}

/// Represents a `<moduleInterface>` element (from `CommonElements_Modular.xsd`).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ModuleInterface {
    #[serde(flatten)]
    pub labels: Glabels,
    #[serde(rename = "fileList")]
    pub file_list: FileList,
    #[serde(rename = "moduleTypeList")]
    pub module_type_list: ModuleTypeList,
    
    #[serde(rename = "@childID")]
    pub child_id: String,
    #[serde(rename = "@type")]
    pub interface_type: String, // xsd:NCName
    #[serde(rename = "@moduleAddressing")]
    pub module_addressing: ModuleAddressingChild,
    
    #[serde(rename = "@minAddress", default, skip_serializing_if = "Option::is_none")]
    pub min_address: Option<String>, // xsd:nonNegativeInteger
    #[serde(rename = "@maxAddress", default, skip_serializing_if = "Option::is_none")]
    pub max_address: Option<String>, // xsd:nonNegativeInteger
    #[serde(rename = "@minPosition", default, skip_serializing_if = "Option::is_none")]
    pub min_position: Option<String>, // xsd:nonNegativeInteger
    #[serde(rename = "@maxPosition", default, skip_serializing_if = "Option::is_none")]
    pub max_position: Option<String>, // xsd:nonNegativeInteger
    #[serde(rename = "@maxCount", default, skip_serializing_if = "Option::is_none")]
    pub max_count: Option<String>, // xsd:nonNegativeInteger
}

// --- Device Profile Modular Types (from ProfileBody_Device_Powerlink_Modular_...) ---

/// Represents a `<connectedModule>` element.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ConnectedModule {
    #[serde(rename = "@childIDRef")]
    pub child_id_ref: String,
    #[serde(rename = "@position")]
    pub position: String, // xsd:positiveInteger
    #[serde(rename = "@address", default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>, // xsd:positiveInteger
}

/// Represents a `<connectedModuleList>` element.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ConnectedModuleList {
    #[serde(rename = "connectedModule", default, skip_serializing_if = "Vec::is_empty")]
    pub connected_module: Vec<ConnectedModule>,
}

/// Represents an `<interface>` element in the *Device* profile.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct InterfaceDevice {
    #[serde(flatten)]
    pub labels: Glabels,
    #[serde(rename = "fileList")]
    pub file_list: FileList,
    #[serde(rename = "connectedModuleList", default, skip_serializing_if = "Option::is_none")]
    pub connected_module_list: Option<ConnectedModuleList>,
    
    #[serde(rename = "@uniqueID")]
    pub unique_id: String, // xsd:ID
    #[serde(rename = "@type")]
    pub interface_type: String, // xsd:NCName
    #[serde(rename = "@maxModules")]
    pub max_modules: String, // xsd:positiveInteger
    #[serde(rename = "@unusedSlots")]
    pub unused_slots: bool,
    #[serde(rename = "@moduleAddressing")]
    pub module_addressing: ModuleAddressingHead,
    #[serde(rename = "@multipleModules", default, skip_serializing_if = "Option::is_none")]
    pub multiple_modules: Option<bool>,
    #[serde(rename = "@identList", default, skip_serializing_if = "Option::is_none")]
    pub ident_list: Option<String>, // xdd:t_Index
    #[serde(rename = "@firmwareList", default, skip_serializing_if = "Option::is_none")]
    pub firmware_list: Option<String>, // xdd:t_Index
}

/// Represents an `<interfaceList>` element in the *Device* profile.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct InterfaceListDevice {
    #[serde(rename = "interface", default, skip_serializing_if = "Vec::is_empty")]
    pub interface: Vec<InterfaceDevice>,
}

/// Represents `<moduleManagement>` in the *Device* profile.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ModuleManagementDevice {
    #[serde(rename = "interfaceList")]
    pub interface_list: InterfaceListDevice,
    #[serde(rename = "moduleInterface", default, skip_serializing_if = "Option::is_none")]
    pub module_interface: Option<ModuleInterface>,
}

// --- Comm Network Profile Modular Types (from ProfileBody_CommunicationNetwork_Modular_...) ---

/// Represents a `<range>` element.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Range {
    #[serde(rename = "@name")]
    pub name: String,
    #[serde(rename = "@baseIndex")]
    pub base_index: String, // xdd:t_Index
    #[serde(rename = "@maxIndex", default, skip_serializing_if = "Option::is_none")]
    pub max_index: Option<String>, // xdd:t_Index
    #[serde(rename = "@maxSubIndex")]
    pub max_sub_index: String, // xdd:t_SubIndex
    #[serde(rename = "@sortMode")]
    pub sort_mode: SortMode,
    #[serde(rename = "@sortNumber")]
    pub sort_number: AddressingAttribute,
    #[serde(rename = "@sortStep", default, skip_serializing_if = "Option::is_none")]
    pub sort_step: Option<String>, // xsd:positiveInteger
    #[serde(rename = "@PDOmapping", default, skip_serializing_if = "Option::is_none")]
    pub pdo_mapping: Option<ObjectPdoMapping>,
}

/// Represents a `<rangeList>` element.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct RangeList {
    #[serde(rename = "range", default, skip_serializing_if = "Vec::is_empty")]
    pub range: Vec<Range>,
}

/// Represents an `<interface>` element in the *Communication* profile.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct InterfaceComm {
    #[serde(rename = "rangeList")]
    pub range_list: RangeList,
    #[serde(rename = "@uniqueIDRef")]
    pub unique_id_ref: String, // xsd:IDREF
}

/// Represents an `<interfaceList>` element in the *Communication* profile.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct InterfaceListComm {
    #[serde(rename = "interface", default, skip_serializing_if = "Vec::is_empty")]
    pub interface: Vec<InterfaceComm>,
}

/// Represents `<moduleManagement>` in the *Communication* profile.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ModuleManagementComm {
    #[serde(rename = "interfaceList")]
    pub interface_list: InterfaceListComm,
}
