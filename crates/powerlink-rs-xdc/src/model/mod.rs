// crates/powerlink-rs-xdc/src/model/mod.rs

//! Internal `serde` data structures that map directly to the XDC XML schema.
//! This file acts as the root module for all model definitions.

#![allow(clippy::pedantic)] // XML schema names are not idiomatic Rust

use serde::{Deserialize, Serialize};
use alloc::vec::Vec;
use alloc::string::String;

// --- Sub-modules ---

pub mod app_layers;
pub mod app_process;
pub mod common;
pub mod header;
pub mod identity;
pub mod net_mgmt;

// --- Public Re-exports from Sub-modules ---
// We only re-export the top-level container and profile structs.
// Other modules will use full paths (e.g., `model::identity::DeviceIdentity`).

pub use header::ProfileHeader;
pub use identity::DeviceIdentity;
pub use app_layers::ApplicationLayers;
pub use app_process::ApplicationProcess;
pub use net_mgmt::NetworkManagement;


/// The root element of an XDC/XDD file.
/// (Based on ISO 15745-1:2005/Amd.1)
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename = "ISO15745ProfileContainer")]
pub struct Iso15745ProfileContainer {
    #[serde(rename = "@xmlns", default)]
    pub xmlns: String,

    #[serde(rename = "@xmlns:xsi", default)]
    pub xmlns_xsi: String,

    #[serde(rename = "@xsi:schemaLocation", default)]
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

    /// This field is only present in the Communication Network Profile.
    #[serde(rename = "NetworkManagement", default, skip_serializing_if = "Option::is_none")]
    pub network_management: Option<NetworkManagement>,
}