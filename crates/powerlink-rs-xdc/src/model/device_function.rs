//! Contains model structs related to `<DeviceFunction>`.
//!
//! (Schema: `ProfileBody_Device_Powerlink.xsd`)

use super::common::{Glabels, LabelChoice};
use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

/// Represents the `<DeviceFunction>` element.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct DeviceFunction {
    #[serde(
        rename = "capabilities",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub capabilities: Option<Capabilities>,
    #[serde(
        rename = "picturesList",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub pictures_list: Option<PicturesList>,
    #[serde(
        rename = "dictionaryList",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub dictionary_list: Option<DictionaryList>,
    #[serde(
        rename = "connectorList",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub connector_list: Option<ConnectorList>,
    #[serde(
        rename = "firmwareList",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub firmware_list: Option<FirmwareList>,
    #[serde(
        rename = "classificationList",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub classification_list: Option<ClassificationList>,
}

/// Represents `<capabilities>`.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Capabilities {
    #[serde(
        rename = "characteristicsList",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub characteristics_list: Vec<CharacteristicsList>,
    #[serde(
        rename = "standardComplianceList",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub standard_compliance_list: Option<StandardComplianceList>,
}

/// Represents `<characteristicsList>`.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct CharacteristicsList {
    #[serde(rename = "category", default, skip_serializing_if = "Option::is_none")]
    pub category: Option<Category>,
    #[serde(
        rename = "characteristic",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub characteristic: Vec<Characteristic>,
}

/// Represents `<category>`.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Category {
    #[serde(flatten)]
    pub labels: Glabels,
}

/// Represents `<characteristic>`.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Characteristic {
    #[serde(rename = "characteristicName")]
    pub characteristic_name: CharacteristicName,
    #[serde(
        rename = "characteristicContent",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub characteristic_content: Vec<CharacteristicContent>,
}

/// Represents `<characteristicName>`.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct CharacteristicName {
    #[serde(rename = "$value", default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<LabelChoice>,
}

/// Represents `<characteristicContent>`.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct CharacteristicContent {
    #[serde(rename = "$value", default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<LabelChoice>,

    #[serde(rename = "@value", default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

/// Represents `<standardComplianceList>`.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct StandardComplianceList {
    #[serde(
        rename = "compliantWith",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
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
    pub range: Option<String>,
}

/// Represents `<picturesList>`.
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
    pub uri: String,
    #[serde(
        rename = "@pictureType",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub picture_type: Option<String>,
    #[serde(rename = "@number", default, skip_serializing_if = "Option::is_none")]
    pub number: Option<String>,
}

/// Represents `<dictionaryList>`.
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
    pub lang: String,
    #[serde(rename = "@dictID")]
    pub dict_id: String,
}

/// Represents `<file>` inside `<dictionary>`.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct DictionaryFile {
    #[serde(rename = "@URI")]
    pub uri: String,
}

/// Represents `<connectorList>`.
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
    pub id: String,
    #[serde(
        rename = "@connectorType",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub connector_type: Option<String>,
    #[serde(
        rename = "@interfaceIDRef",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub interface_id_ref: Option<String>,
    #[serde(rename = "@posX", default, skip_serializing_if = "Option::is_none")]
    pub pos_x: Option<String>,
    #[serde(rename = "@posY", default, skip_serializing_if = "Option::is_none")]
    pub pos_y: Option<String>,
    #[serde(
        rename = "@positioning",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub positioning: Option<String>,
}

/// Represents `<firmwareList>`.
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
    pub uri: String,
    #[serde(rename = "@deviceRevisionNumber")]
    pub device_revision_number: String,
    #[serde(
        rename = "@buildDate",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub build_date: Option<String>,
}

/// Represents `<classificationList>`.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ClassificationList {
    #[serde(
        rename = "classification",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub classification: Vec<Classification>,
}

/// Represents `<classification>`.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Classification {
    #[serde(rename = "@value")]
    pub value: String,
}
