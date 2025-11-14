// crates/powerlink-rs-xdc/src/model.rs

//! Internal `serde` data structures that map directly to the XDC XML schema.
//! These are used for raw deserialization and serialization.

#![allow(clippy::pedantic)] // XML schema names are not idiomatic Rust

use serde::{Deserialize, Serialize};
use alloc::vec::Vec;
use alloc::string::String;

/// The root element of an XDC/XDD file.
/// (Based on ISO 15745-1:2005/Amd.1)
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename = "ISO15745ProfileContainer")]
pub struct Iso15745ProfileContainer {
    #[serde(rename = "@xmlns")]
    pub xmlns: String,

    #[serde(rename = "@xmlns:xsi")]
    pub xmlns_xsi: String,

    #[serde(rename = "@xsi:schemaLocation")]
    pub xsi_schema_location: String,

    #[serde(rename = "ISO15745Profile", default)]
    pub profile: Vec<Iso15745Profile>,
}

impl Default for Iso15745ProfileContainer {
    fn default() -> Self {
        Self {
            xmlns: "http://www.ethernet-powerlink.org".into(),
            xmlns_xsi: "http://www.w3.org/2001/XMLSchema-instance".into(),
            xsi_schema_location: "http://www.ethernet-powerlink.org Powerlink_Main.xsd".into(),
            profile: Vec::new(),
        }
    }
}

/// Represents either the Device Profile or the Communication Network Profile.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Iso15745Profile {
    #[serde(rename = "ProfileHeader")]
    pub profile_header: ProfileHeader,
    
    #[serde(rename = "ProfileBody")]
    pub profile_body: ProfileBody,
}

/// Header containing metadata. We don't parse its contents yet, but it must
/// be present for serde to succeed.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ProfileHeader {
    // We can add fields here if we need them later (e.g., ProfileIdentification)
}

/// The main body containing either device or communication data.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ProfileBody {
    /// Used to identify which ProfileBody this is (e.g. "ProfileBody_Device_Powerlink").
    #[serde(rename = "@xsi:type", default, skip_serializing_if = "Option::is_none")]
    pub xsi_type: Option<String>,

    /// This field is only present in the Communication Network Profile.
    #[serde(rename = "ApplicationLayers", default, skip_serializing_if = "Option::is_none")]
    pub application_layers: Option<ApplicationLayers>,

    /// This field is only present in the Device Profile.
    #[serde(rename = "DeviceIdentity", default, skip_serializing_if = "Option::is_none")]
    pub device_identity: Option<DeviceIdentity>,

    /// This field is only present in the Device Profile.
    #[serde(rename = "ApplicationProcess", default, skip_serializing_if = "Option::is_none")]
    pub application_process: Option<ApplicationProcess>,
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

    /// This attribute references a Parameter's uniqueID in the ApplicationProcess.
    #[serde(rename = "@uniqueIDRef", default, skip_serializing_if = "Option::is_none")]
    pub unique_id_ref: Option<String>,

    /// The POWERLINK data type (e.g., "0006" for Unsigned16).
    #[serde(rename = "@dataType", default, skip_serializing_if = "Option::is_none")]
    pub data_type: Option<String>,

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

    /// This attribute references a Parameter's uniqueID in the ApplicationProcess.
    #[serde(rename = "@uniqueIDRef", default, skip_serializing_if = "Option::is_none")]
    pub unique_id_ref: Option<String>,

    /// The POWERLINK data type (e.g., "0006" for Unsigned16).
    #[serde(rename = "@dataType", default, skip_serializing_if = "Option::is_none")]
    pub data_type: Option<String>,
}

// --- NEW STRUCTS for ApplicationProcess ---

/// Represents the `<ApplicationProcess>` block (EPSG 311, 7.4.7).
/// This contains the device parameters, which are the source of default values.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ApplicationProcess {
    #[serde(rename = "parameterList", default, skip_serializing_if = "Option::is_none")]
    pub parameter_list: Option<ParameterList>,
    // Other lists like dataTypeList, functionTypeList, etc., can be added here if needed.
}

/// Represents `<parameterList>` (EPSG 311, 7.4.7.7).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ParameterList {
    #[serde(rename = "parameter", default, skip_serializing_if = "Vec::is_empty")]
    pub parameter: Vec<Parameter>,
}

/// Represents a `<parameter>` (EPSG 311, 7.4.7.7.2).
/// We only capture the fields relevant for default value resolution.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Parameter {
    /// The unique ID (e.g., "Param1_Vendor_Specific") that `uniqueIDRef` points to.
    #[serde(rename = "@uniqueID")]
    pub unique_id: String,
    
    /// The `defaultValue` element, if present.
    #[serde(rename = "defaultValue", default, skip_serializing_if = "Option::is_none")]
    pub default_value: Option<Value>,

    /// The `actualValue` element, if present (less common for defaults).
    #[serde(rename = "actualValue", default, skip_serializing_if = "Option::is_none")]
    pub actual_value: Option<Value>,
    
    // Other attributes like access, support, etc., can be added here.
}

/// Represents a simple value-holding element like `<defaultValue value="0x01"/>`.
/// (EPSG 311, 7.4.7.7.2.4, 7.4.7.7.2.5).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Value {
    #[serde(rename = "@value")]
    pub value: String,
}