// crates/powerlink-rs-xdc/src/model/header.rs

//! Contains model structs related to the `<ProfileHeader>`.
//! (Schema: `ISO15745ProfileContainer.xsd`)

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

// --- Enums and Structs for ProfileHeader ---

/// Represents the `<ProfileClassID>` element (from XSD `t_ProfileClassID`).
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum ProfileClassId {
    // Values from ISO15745ProfileContainer.xsd
    AIP,
    Process,
    InformationExchange,
    Resource,
    Device,
    CommunicationNetwork,
    Equipment,
    Human,
    Material,
}

impl Default for ProfileClassId {
    fn default() -> Self {
        // All fields in a struct with `#[derive(Default)]` must implement
        // Default. `ProfileHeader` derives Default. While there's no
        // "correct" default, `DeviceProfile` is the most common for XDC.
        Self::Device
    }
}

/// Represents the `<ISO15745Reference>` element (from XSD `t_ISO15745Reference`).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Iso15745Reference {
    #[serde(rename = "ISO15745Part")]
    pub iso15745_part: u32,
    #[serde(rename = "ISO15745Edition")]
    pub iso15745_edition: u32,
    #[serde(rename = "ProfileTechnology")]
    pub profile_technology: String,
}

/// Represents the `<IASInterfaceType>` element wrapper (from XSD `t_IASInterfaceType`).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct IasInterfaceType {
    #[serde(rename = "IASInterfaceType")]
    pub ias_interface_type: String,
}

/// Represents `<ProfileHeader>` (from XSD `t_ProfileHeader`).
/// This contains metadata identifying the profile.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ProfileHeader {
    #[serde(rename = "ProfileIdentification")]
    pub profile_identification: String,

    #[serde(rename = "ProfileRevision")]
    pub profile_revision: String,

    #[serde(rename = "ProfileName")]
    pub profile_name: String,

    #[serde(rename = "ProfileSource")]
    pub profile_source: String,

    #[serde(rename = "ProfileClassID")]
    pub profile_class_id: ProfileClassId,

    /// Stored as a string as `xs:date` (e.g., "2024-01-01")
    #[serde(
        rename = "ProfileDate",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub profile_date: Option<String>,

    #[serde(
        rename = "AdditionalInformation",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub additional_information: Option<String>,

    // Per schema, ISO15745Reference is mandatory, but quick-xml needs default
    // if it's inside an optional container. Let's make it optional for robustness.
    #[serde(
        rename = "ISO15745Reference",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub iso15745_reference: Option<Iso15745Reference>,

    #[serde(
        rename = "IASInterfaceType",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub ias_interface_type: Vec<IasInterfaceType>, // Schema says maxOccurs="unbounded"
}
