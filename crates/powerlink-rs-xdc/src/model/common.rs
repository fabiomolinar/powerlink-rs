//! Defines common helper structs and enums shared across the XDC schema models.
//!
//! Based on `CommonElements.xsd`.

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

// --- Helper Functions for Serde Defaults ---

/// Returns `true`. Used for `#[serde(default = "bool_true")]`.
pub fn bool_true() -> bool {
    true
}

/// Checks if a boolean is true. Used for `#[serde(skip_serializing_if = "is_true")]`.
pub(super) fn is_true(b: &bool) -> bool {
    *b
}

/// Returns `false`. Used for `#[serde(default = "bool_false")]`.
pub fn bool_false() -> bool {
    false
}

/// Checks if a boolean is false. Used for `#[serde(skip_serializing_if = "is_false")]`.
pub(super) fn is_false(b: &bool) -> bool {
    !*b
}

// --- Label and Description Types ---

/// Represents a localized `<label>` element.
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Label {
    #[serde(rename = "@lang")]
    pub lang: String,
    #[serde(rename = "$value")]
    pub value: String,
}

/// Represents a localized `<description>` element.
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Description {
    #[serde(rename = "@lang")]
    pub lang: String,
    #[serde(rename = "@URI", default, skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    #[serde(rename = "$value")]
    pub value: String,
}

/// Represents a reference to an external label definition.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LabelRef {
    #[serde(rename = "@dictID")]
    pub dict_id: String,
    #[serde(rename = "@textID")]
    pub text_id: String,
    #[serde(rename = "$value")]
    pub value: String,
}

/// Represents a reference to an external description.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DescriptionRef {
    #[serde(rename = "@dictID")]
    pub dict_id: String,
    #[serde(rename = "@textID")]
    pub text_id: String,
    #[serde(rename = "$value")]
    pub value: String,
}

/// Enum wrapper for the `g_labels` choice group in the schema.
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

/// Represents the `g_labels` group, containing a list of labels or descriptions.
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Glabels {
    #[serde(rename = "$value", default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<LabelChoice>,
}

// --- Common Helper Structs ---

/// A string value with a `readOnly` attribute (defaults to true).
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct ReadOnlyString {
    #[serde(
        rename = "@readOnly",
        default = "bool_true",
        skip_serializing_if = "is_true"
    )]
    pub read_only: bool,
    #[serde(rename = "$value")]
    pub value: String,
}

/// An instance name string with a `readOnly` attribute (defaults to false).
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct InstanceName {
    #[serde(rename = "@readOnly", default, skip_serializing_if = "is_false")]
    pub read_only: bool,
    #[serde(rename = "$value")]
    pub value: String,
}

/// A combination of `Glabels` and a `readOnly` attribute.
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct AttributedGlabels {
    #[serde(rename = "$value", default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<LabelChoice>,
    #[serde(
        rename = "@readOnly",
        default = "bool_true",
        skip_serializing_if = "is_true"
    )]
    pub read_only: bool,
}

/// Represents a reference to a data type via unique ID.
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct DataTypeIDRef {
    #[serde(rename = "@uniqueIDRef")]
    pub unique_id_ref: String,
}