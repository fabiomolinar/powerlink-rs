// crates/powerlink-rs-xdc/src/model/identity.rs

//! Contains model structs related to `<DeviceIdentity>`.
//! (Schema: `ProfileBody_Device_Powerlink.xsd`)

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};
// Removed unused import `is_false`
use super::common::{AttributedGlabels, InstanceName, ReadOnlyString, bool_true, is_true};

/// Represents a `<version>` element within `<DeviceIdentity>`.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Version {
    #[serde(rename = "@versionType")]
    pub version_type: String,

    #[serde(
        rename = "@readOnly",
        default = "bool_true",
        skip_serializing_if = "is_true"
    )]
    pub read_only: bool, // Added from schema

    #[serde(rename = "$value")]
    pub value: String,
}

/// Represents the `<DeviceIdentity>` block in the Device Profile.
/// (Updated for Task 2)
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct DeviceIdentity {
    #[serde(rename = "vendorName")]
    pub vendor_name: ReadOnlyString,

    #[serde(rename = "vendorID", default, skip_serializing_if = "Option::is_none")]
    pub vendor_id: Option<ReadOnlyString>,

    #[serde(
        rename = "vendorText",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub vendor_text: Option<AttributedGlabels>, // New

    #[serde(
        rename = "deviceFamily",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub device_family: Option<AttributedGlabels>, // New

    #[serde(
        rename = "productFamily",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub product_family: Option<ReadOnlyString>, // New

    #[serde(rename = "productName")]
    pub product_name: ReadOnlyString,

    #[serde(rename = "productID", default, skip_serializing_if = "Option::is_none")]
    pub product_id: Option<ReadOnlyString>,

    #[serde(
        rename = "productText",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub product_text: Option<AttributedGlabels>, // New

    #[serde(rename = "orderNumber", default, skip_serializing_if = "Vec::is_empty")]
    pub order_number: Vec<ReadOnlyString>, // New

    #[serde(rename = "version", default, skip_serializing_if = "Vec::is_empty")]
    pub version: Vec<Version>,

    #[serde(rename = "buildDate", default, skip_serializing_if = "Option::is_none")]
    pub build_date: Option<String>, // xsd:date, New

    #[serde(
        rename = "specificationRevision",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub specification_revision: Option<ReadOnlyString>, // New

    #[serde(
        rename = "instanceName",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub instance_name: Option<InstanceName>, // New
}
