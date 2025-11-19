//! Contains model structs related to the `<ProfileHeader>`.
//!
//! (Schema: `ISO15745ProfileContainer.xsd`)

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

/// Enum defining the classification of the profile.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum ProfileClassId {
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
        Self::Device
    }
}

/// References the specific part and edition of the ISO 15745 standard.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Iso15745Reference {
    #[serde(rename = "ISO15745Part")]
    pub iso15745_part: u32,
    #[serde(rename = "ISO15745Edition")]
    pub iso15745_edition: u32,
    #[serde(rename = "ProfileTechnology")]
    pub profile_technology: String,
}

/// Wrapper for the Interface type definition.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct IasInterfaceType {
    #[serde(rename = "IASInterfaceType")]
    pub ias_interface_type: String,
}

/// Metadata header for the profile, containing identification and revision info.
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
    pub ias_interface_type: Vec<IasInterfaceType>,
}
