// src/model.rs

//! Internal `serde` data structures that map directly to the XDC XML schema.
//! These are used for raw deserialization and serialization.

#![allow(clippy::pedantic)] // XML schema names are not idiomatic Rust

use serde::{Deserialize, Serialize};
use alloc::vec::Vec;
use alloc::string::String;

/// The root element of an XDC/XDD file.
/// (Based on ISO 15745-1:2005/Amd.1)
#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename = "ISO15745ProfileContainer")]
pub struct Iso15745ProfileContainer {
    #[serde(rename = "ISO15745Profile", default)]
    pub profile: Vec<Iso15745Profile>,
}

/// Represents either the Device Profile or the Communication Network Profile.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Iso15745Profile {
    #[serde(rename = "ProfileHeader")]
    pub profile_header: ProfileHeader,
    
    #[serde(rename = "ProfileBody")]
    pub profile_body: ProfileBody,
}

/// Header containing metadata. We don't parse its contents, but it must
/// be present for serde to succeed.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ProfileHeader {
    // We can add fields here if we need them later (e.g., ProfileIdentification)
}

/// The main body containing either device or communication data.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ProfileBody {
    /// This field is only present in the Communication Network Profile.
    #[serde(rename = "ApplicationLayers", default, skip_serializing_if = "Option::is_none")]
    pub application_layers: Option<ApplicationLayers>,

    /// This field is only present in the Device Profile.
    #[serde(rename = "DeviceIdentity", default, skip_serializing_if = "Option::is_none")]
    pub device_identity: Option<DeviceIdentity>,
    
    /// Used to identify which ProfileBody this is.
    #[serde(rename = "@type", default, skip_serializing_if = "Option::is_none")]
    pub xsi_type: Option<String>,
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

/// Contains the ObjectList.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ApplicationLayers {
    #[serde(rename = "ObjectList")]
    pub object_list: ObjectList,
}

/// A list of all Object Dictionary entries.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ObjectList {
    #[serde(rename = "Object", default)]
    pub object: Vec<Object>,
}

/// Represents an Object Dictionary index (e.g., <Object index="1F22"...>).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Object {
    /// The OD index as a hex string (e.g., "1F22").
    #[serde(rename = "@index")]
    pub index: String,
    
    /// The object type (e.g., "9" for RECORD).
    #[serde(rename = "@objectType")]
    pub object_type: String,

    /// A list of SubObjects (e.g., <SubObject subIndex="01"...>).
    #[serde(rename = "SubObject", default)]
    pub sub_object: Vec<SubObject>,
}

/// Represents an Object Dictionary sub-index.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct SubObject {
    /// The OD sub-index as a hex string (e.g., "01").
    #[serde(rename = "@subIndex")]
    pub sub_index: String,
    
    /// The `actualValue` is the key data for an XDC file.
    #[serde(rename = "@actualValue", default, skip_serializing_if = "Option::is_none")]
    pub actual_value: Option<String>,
    
    /// The `defaultValue` is the key data for an XDD file.
    #[serde(rename = "@defaultValue", default, skip_serializing_if = "Option::is_none")]
    pub default_value: Option<String>,
}