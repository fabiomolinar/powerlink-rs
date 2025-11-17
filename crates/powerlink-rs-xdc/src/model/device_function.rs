// crates/powerlink-rs-xdc/src/model/device_function.rs

//! Contains model structs related to `<DeviceFunction>`.
//! (Schema: `ProfileBody_Device_Powerlink.xsd`)

#![allow(clippy::pedantic)] // XML schema names are not idiomatic Rust

use serde::{Deserialize, Serialize};
use alloc::vec::Vec;
use alloc::string::String;
use super::common::Glabels;

/// Represents the `<DeviceFunction>` element (EPSG DS 311, 7.4.6).
/// This element is mandatory (minOccurs=1) and can be unbounded.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct DeviceFunction {
    #[serde(rename = "capabilities", default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Capabilities>,
    
    #[serde(rename = "picturesList", default, skip_serializing_if = "Option::is_none")]
    pub pictures_list: Option<PicturesList>,
    
    #[serde(rename = "dictionaryList", default, skip_serializing_if = "Option::is_none")]
    pub dictionary_list: Option<DictionaryList>,
    
    #[serde(rename = "connectorList", default, skip_serializing_if = "Option::is_none")]
    pub connector_list: Option<ConnectorList>,
    
    #[serde(rename = "firmwareList", default, skip_serializing_if = "Option::is_none")]
    pub firmware_list: Option<FirmwareList>,
    
    #[serde(rename = "classificationList", default, skip_serializing_if = "Option::is_none")]
    pub classification_list: Option<ClassificationList>,
}

/// Represents `<capabilities>` (EPSG DS 311, 7.4.6.2).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Capabilities {
    #[serde(rename = "characteristicsList", default, skip_serializing_if = "Vec::is_empty")]
    pub characteristics_list: Vec<CharacteristicsList>,
    
    #[serde(rename = "standardComplianceList", default, skip_serializing_if = "Option::is_none")]
    pub standard_compliance_list: Option<StandardComplianceList>,
}

/// Represents `<characteristicsList>` (EPSG DS 311, 7.4.6.2.2).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct CharacteristicsList {
    #[serde(rename = "category", default, skip_serializing_if = "Option::is_none")]
    pub category: Option<Category>,
    
    #[serde(rename = "characteristic", default, skip_serializing_if = "Vec::is_empty")]
    pub characteristic: Vec<Characteristic>,
}

/// Represents `<category>` within `<characteristicsList>`.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Category {
    #[serde(flatten)]
    pub labels: Glabels,
}

/// Represents `<characteristic>` (EPSG DS 311, 7.4.6.2.2.2).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Characteristic {
    #[serde(rename = "characteristicName")]
    pub characteristic_name: CharacteristicName,
    
    #[serde(rename = "characteristicContent", default, skip_serializing_if = "Vec::is_empty")]
    pub characteristic_content: Vec<CharacteristicContent>,
}

/// Represents `<characteristicName>` (EPSG DS 311, 7.4.6.2.2.3).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct CharacteristicName {
    #[serde(flatten)]
    pub labels: Glabels,
}

/// Represents `<characteristicContent>` (EPSG DS 311, 7.4.6.2.2.4).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct CharacteristicContent {
    #[serde(flatten)]
    pub labels: Glabels,
}

/// Represents `<standardComplianceList>` (EPSG DS 311, 7.4.6.2.2.5).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct StandardComplianceList {
    #[serde(rename = "compliantWith", default, skip_serializing_if = "Vec::is_empty")]
    pub compliant_with: Vec<CompliantWith>,
}

/// Represents `<compliantWith>`.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct CompliantWith {
    #[serde(flatten)]
    pub labels: Glabels,
    
    #[serde(rename = "@name")]
    pub name: String,
    
    #[serde(rename = "@range", default, skip_serializing_if = "Option::is_none")]
    pub range: Option<String>, // "international" (default) or "internal"
}

/// Represents `<picturesList>` (EPSG DS 311, 7.4.6.3).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct PicturesList {
    #[serde(rename = "picture", default, skip_serializing_if = "Vec::is_empty")]
    pub picture: Vec<Picture>,
}

/// Represents `<picture>`.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Picture {
    #[serde(flatten)]
    pub labels: Glabels,
    
    #[serde(rename = "@URI")]
    pub uri: String, // xsd:anyURI
    
    #[serde(rename = "@type", default, skip_serializing_if = "Option::is_none")]
    pub picture_type: Option<String>, // "frontPicture", "icon", "additional", "none" (default)
    
    #[serde(rename = "@number", default, skip_serializing_if = "Option::is_none")]
    pub number: Option<String>, // xsd:unsignedInt
}

/// Represents `<dictionaryList>` (EPSG DS 311, 7.4.6.4).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct DictionaryList {
    #[serde(rename = "dictionary", default, skip_serializing_if = "Vec::is_empty")]
    pub dictionary: Vec<Dictionary>,
}

/// Represents `<dictionary>`.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Dictionary {
    #[serde(rename = "file")]
    pub file: DictionaryFile,
    
    #[serde(rename = "@lang")]
    pub lang: String, // xsd:language
    
    #[serde(rename = "@dictID")]
    pub dict_id: String, // xsd:token
}

/// Represents `<file>` within `<dictionary>`.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct DictionaryFile {
    #[serde(rename = "@URI")]
    pub uri: String, // xsd:anyURI
}

/// Represents `<connectorList>` (EPSG DS 311, 7.4.6.5).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ConnectorList {
    #[serde(rename = "connector", default, skip_serializing_if = "Vec::is_empty")]
    pub connector: Vec<Connector>,
}

/// Represents `<connector>`.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Connector {
    #[serde(flatten)]
    pub labels: Glabels,
    
    #[serde(rename = "@id")]
    pub id: String, // xsd:string
    
    #[serde(rename = "@posX", default, skip_serializing_if = "Option::is_none")]
    pub pos_x: Option<String>, // xsd:nonNegativeInteger
    
    #[serde(rename = "@posY", default, skip_serializing_if = "Option::is_none")]
    pub pos_y: Option<String>, // xsd:nonNegativeInteger
    
    #[serde(rename = "@connectorType", default, skip_serializing_if = "Option::is_none")]
    pub connector_type: Option<String>, // default "POWERLINK"
    
    #[serde(rename = "@interfaceIDRef", default, skip_serializing_if = "Option::is_none")]
    pub interface_id_ref: Option<String>,
    
    #[serde(rename = "@positioning", default, skip_serializing_if = "Option::is_none")]
    pub positioning: Option<String>, // "remote" (default), "localAbove", etc.
}

/// Represents `<firmwareList>` (EPSG DS 311, 7.4.6.6).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct FirmwareList {
    #[serde(rename = "firmware", default, skip_serializing_if = "Vec::is_empty")]
    pub firmware: Vec<Firmware>,
}

/// Represents `<firmware>`.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Firmware {
    #[serde(flatten)]
    pub labels: Glabels,
    
    #[serde(rename = "@URI")]
    pub uri: String, // xsd:anyURI
    
    #[serde(rename = "@deviceRevisionNumber")]
    pub device_revision_number: String, // xsd:nonNegativeInteger
    
    #[serde(rename = "@buildDate", default, skip_serializing_if = "Option::is_none")]
    pub build_date: Option<String>, // xsd:dateTime
}

/// Represents `<classificationList>` (EPSG DS 311, 7.4.6.7).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ClassificationList {
    #[serde(rename = "classification", default, skip_serializing_if = "Vec::is_empty")]
    pub classification: Vec<Classification>,
}

/// Represents `<classification>`.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Classification {
    #[serde(rename = "$value")]
    pub value: String, // xsd:NMTOKEN, e.g., "Controller", "IO", "Drive"
}