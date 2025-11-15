// crates/powerlink-rs-xdc/src/model/common.rs

//! Contains common helper structs and enums from CommonElements.xsd.

use serde::{Deserialize, Serialize};
use alloc::vec::Vec;
use alloc::string::String;

// --- Helper Functions for serde(default) ---

/// Helper function for `#[serde(default)]` on bool fields that should default to `true`.
pub(super) fn bool_true() -> bool {
    true
}

/// Helper function for `#[serde(skip_serializing_if = "is_true")]`
pub(super) fn is_true(b: &bool) -> bool {
    *b
}

/// Helper function for `#[serde(skip_serializing_if = "is_false")]`
pub(super) fn is_false(b: &bool) -> bool {
    !*b
}

// --- Structs for g_labels (CommonElements.xsd) ---

/// Represents `<label lang="en">Value</label>`
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Label {
    #[serde(rename = "@lang")]
    pub lang: String,
    #[serde(rename = "$value")]
    pub value: String,
}

/// Represents `<description lang="en" URI="...">Value</description>`
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Description {
    #[serde(rename = "@lang")]
    pub lang: String,
    #[serde(rename = "@URI", default, skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    #[serde(rename = "$value")]
    pub value: String,
}

/// Represents `<labelRef dictID="..." textID="...">Value</labelRef>`
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LabelRef {
    #[serde(rename = "@dictID")]
    pub dict_id: String,
    #[serde(rename = "@textID")]
    pub text_id: String,
    #[serde(rename = "$value")]
    pub value: String, // xsd:anyURI
}

/// Represents `<descriptionRef dictID="..." textID="...">Value</descriptionRef>`
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DescriptionRef {
    #[serde(rename = "@dictID")]
    pub dict_id: String,
    #[serde(rename = "@textID")]
    pub text_id: String,
    #[serde(rename = "$value")]
    pub value: String, // xsd:anyURI
}

/// Represents the `xsd:choice` inside `g_labels`
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum LabelChoice {
    #[serde(rename = "label")]
    Label(Label),
    #[serde(rename = "description")]
    Description(Description),
    #[serde(rename = "labelRef")]
    LabelRef(LabelRef),
    #[serde(rename = "descriptionRef")]
    DescriptionRef(DescriptionRef),
}

/// Represents the `g_labels` group from CommonElements.xsd
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Glabels {
    // This captures the <xsd:choice maxOccurs="unbounded">
    #[serde(rename = "$value", default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<LabelChoice>,
}

// --- Common Helper Structs for DeviceIdentity / ApplicationProcess ---

/// Represents `<vendorName readOnly="true">Value</vendorName>`
/// Also used for `productFamily`, `productName`, `productID`, `specificationRevision`
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct ReadOnlyString {
    #[serde(rename = "@readOnly", default = "bool_true", skip_serializing_if = "is_true")]
    pub read_only: bool,
    #[serde(rename = "$value")]
    pub value: String,
}

/// Represents `<instanceName readOnly="false">Value</instanceName>`
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct InstanceName {
    #[serde(rename = "@readOnly", default, skip_serializing_if = "is_false")]
    pub read_only: bool,
    #[serde(rename = "$value")]
    pub value: String,
}

/// Represents `<vendorText>`, `<deviceFamily>`, and `<productText>`
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct AttributedGlabels {
    #[serde(flatten)]
    pub labels: Glabels,
    #[serde(rename = "@readOnly", default = "bool_true", skip_serializing_if = "is_true")]
    pub read_only: bool,
}

/// Represents `<dataTypeIDRef>` (EPSG 311, 7.4.7.4.3.3).
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct DataTypeIDRef {
    #[serde(rename = "@uniqueIDRef")]
    pub unique_id_ref: String,
}
