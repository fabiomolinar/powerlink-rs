//! Internal `serde` data structures that map directly to the XDC XML schema.
//!
//! This module defines the raw structure of an XDC file as defined by the EPSG DS 311
//! XSD schemas. These structs are annotated with `serde` attributes to facilitate
//! parsing via `quick-xml` and are not intended for direct public use.

#![allow(clippy::pedantic)] // XML schema naming conventions differ from Rust

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

pub mod app_layers;
pub mod app_process;
pub mod common;
pub mod device_function;
pub mod device_manager;
pub mod header;
pub mod identity;
pub mod modular;
pub mod net_mgmt;

// Re-export key components for internal use
pub use app_layers::ApplicationLayers;
pub use app_process::ApplicationProcess;
pub use device_function::DeviceFunction;
pub use device_manager::DeviceManager;
pub use header::ProfileHeader;
pub use identity::DeviceIdentity;
pub use net_mgmt::NetworkManagement;

/// The root element of an XDC/XDD file.
///
/// Represents the `<ISO15745ProfileContainer>` element defined in `ISO15745ProfileContainer.xsd`.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename = "ISO15745ProfileContainer")]
pub struct Iso15745ProfileContainer {
    #[serde(rename = "@xmlns", default)]
    pub xmlns: String,

    #[serde(rename = "@xmlns:xsi", default)]
    pub xmlns_xsi: String,

    #[serde(rename = "@xsi:schemaLocation", default)]
    pub xsi_schema_location: String,

    /// A container can hold multiple profiles (usually one Device Profile and one Communication Profile).
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

/// Represents a single profile within the container.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Iso15745Profile {
    #[serde(rename = "ProfileHeader")]
    pub profile_header: ProfileHeader,

    #[serde(rename = "ProfileBody")]
    pub profile_body: ProfileBody,
}

/// The main body of a profile, containing either Device or Communication specific data.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ProfileBody {
    /// Identifies the type of profile body (e.g., "ProfileBody_Device_Powerlink").
    #[serde(rename = "@xsi:type", default, skip_serializing_if = "Option::is_none")]
    pub xsi_type: Option<String>,

    /// Present in Communication Network Profiles.
    #[serde(
        rename = "ApplicationLayers",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub application_layers: Option<ApplicationLayers>,

    /// Present in Device Profiles.
    #[serde(
        rename = "DeviceIdentity",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub device_identity: Option<DeviceIdentity>,

    /// Present in Device Profiles (manages indicators like LEDs).
    #[serde(
        rename = "DeviceManager",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub device_manager: Option<DeviceManager>,

    /// Present in Device Profiles (describes capabilities).
    #[serde(
        rename = "DeviceFunction",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub device_function: Vec<DeviceFunction>,

    /// Present in Device Profiles (defines parameters and types).
    #[serde(
        rename = "ApplicationProcess",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub application_process: Option<ApplicationProcess>,

    /// Present in Communication Network Profiles (defines NMT features).
    #[serde(
        rename = "NetworkManagement",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub network_management: Option<NetworkManagement>,
}
