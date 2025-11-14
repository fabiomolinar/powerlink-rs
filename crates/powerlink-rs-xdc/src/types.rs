// crates/powerlink-rs-xdc/src/types.rs

//! Public, ergonomic data structures for representing a parsed XDC file.

use alloc::string::String;
use alloc::vec::Vec;

// --- Root XDC Structure ---

/// Represents a fully parsed and resolved XDC/XDD file.
///
/// This is the main public struct, providing ergonomic access to all
/// data contained within the XML file.
#[derive(Debug, Default)]
pub struct XdcFile {
    /// Metadata from the `<ProfileHeader>` block.
    pub header: ProfileHeader,
    
    /// Information from the `<DeviceIdentity>` block.
    pub identity: Identity,
    
    /// Information from the `<NetworkManagement>` block.
    /// This is `None` if the file is a standard Device Profile (XDD).
    pub network_management: Option<NetworkManagement>,
    
    /// The complete Object Dictionary for the device.
    pub object_dictionary: ObjectDictionary,
    
    // We can add ApplicationProcess here later if needed.
}

// --- Profile Header ---

/// Represents the `<ProfileHeader>` block, containing file metadata.
#[derive(Debug, Default)]
pub struct ProfileHeader {
    /// `<ProfileIdentification>`
    pub identification: String,
    /// `<ProfileRevision>`
    pub revision: String,
    /// `<ProfileName>`
    pub name: String,
    /// `<ProfileSource>`
    pub source: String,
    /// `<ProfileDate>`
    pub date: Option<String>,
}

// --- Device Identity ---

/// Represents the `<DeviceIdentity>` block.
#[derive(Debug, Default)]
pub struct Identity {
    /// `<vendorName>`
    pub vendor_name: Option<String>,
    /// `<vendorID>` (as a u32, parsed from hex)
    pub vendor_id: u32,
    /// `<productName>`
    pub product_name: Option<String>,
    /// `<productID>` (as a u32, parsed from hex)
    pub product_id: u32,
    /// All `<version>` elements.
    pub versions: Vec<Version>,
}

/// Represents a `<version>` element.
#[derive(Debug, Default)]
pub struct Version {
    /// `@versionType`
    pub version_type: String,
    /// `@value`
    pub value: String,
}

// --- Network Management ---

/// Represents the `<NetworkManagement>` block from the Comm Profile.
#[derive(Debug, Default)]
pub struct NetworkManagement {
    pub general_features: GeneralFeatures,
    pub mn_features: Option<MnFeatures>,
    pub cn_features: Option<CnFeatures>,
    pub diagnostic: Option<Diagnostic>,
    // Add deviceCommissioning later if needed
}

/// Represents `<GeneralFeatures>`.
#[derive(Debug, Default)]
pub struct GeneralFeatures {
    /// `@DLLFeatureMN`
    pub dll_feature_mn: Option<bool>,
    /// `@NMTBootTimeNotActive`
    pub nmt_boot_time_not_active: Option<String>,
    // Add other features as needed
}

/// Represents `<MNFeatures>`.
#[derive(Debug, Default)]
pub struct MnFeatures {
    /// `@NMTMNMaxCycInSync`
    pub nmt_mn_max_cyc_in_sync: Option<String>,
    /// `@NMTMNPResMax`
    pub nmt_mn_pres_max: Option<String>,
    // Add other features as needed
}

/// Represents `<CNFeatures>`.
#[derive(Debug, Default)]
pub struct CnFeatures {
    /// `@NMTCNPreOp2ToReady2Op`
    pub nmt_cn_pre_op2_to_ready2_op: Option<String>,
    /// `@NMTCNDNA`
    pub nmt_cn_dna: Option<bool>, // Simplified from the model's enum for now
    // Add other features as needed
}

/// Represents `<Diagnostic>` capabilities.
#[derive(Debug, Default)]
pub struct Diagnostic {
    /// All defined `<Error>` elements.
    pub errors: Vec<ErrorDefinition>,
}

/// Represents one `<Error>` in the `<ErrorList>`.
#[derive(Debug, Default)]
pub struct ErrorDefinition {
    pub name: Option<String>,
    pub label: Option<String>,
    pub description: Option<String>,
    pub error_type: Option<String>,
    pub value: Option<String>,
}

// --- Object Dictionary ---

/// Access types for an Object Dictionary entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectAccessType {
    ReadOnly,
    WriteOnly,
    ReadWrite,
    Constant,
}

/// PDO mapping capabilities for an Object Dictionary entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectPdoMapping {
    No,
    Default,
    Optional,
    Tpdo,
    Rpdo,
}

/// Represents the complete `<ObjectList>` (Object Dictionary).
#[derive(Debug, Default)]
pub struct ObjectDictionary {
    pub objects: Vec<Object>,
}

/// Represents a single `<Object>` (an OD Index).
#[derive(Debug, Default)]
pub struct Object {
    /// `@index` (as a u16, parsed from hex)
    pub index: u16,
    
    // --- Metadata ---
    /// `@name`
    pub name: String,
    /// `@objectType` (e.g., "7" for VAR, "9" for RECORD)
    pub object_type: String,
    /// `@dataType` (as a hex string, e.g., "0006")
    pub data_type: Option<String>,
    /// `@lowLimit`
    pub low_limit: Option<String>,
    /// `@highLimit`
    pub high_limit: Option<String>,
    /// `@accessType`
    pub access_type: Option<ObjectAccessType>,
    /// `@PDOmapping`
    pub pdo_mapping: Option<ObjectPdoMapping>,
    /// `@objFlags`
    pub obj_flags: Option<String>,
    
    // --- Value ---
    /// The resolved data for this object, from `actualValue` or `defaultValue`.
    /// This is `None` for RECORD types (where data is in sub-objects).
    pub data: Option<Vec<u8>>,
    
    // --- Children ---
    /// All `<SubObject>` children.
    pub sub_objects: Vec<SubObject>,
}

/// Represents a `<SubObject>` (an OD Sub-Index).
#[derive(Debug, Default)]
pub struct SubObject {
    /// `@subIndex` (as a u8, parsed from hex)
    pub sub_index: u8,
    
    // --- Metadata ---
    /// `@name`
    pub name: String,
    /// `@objectType` (e.D., "7" for VAR)
    pub object_type: String,
    /// `@dataType` (as a hex string, e.g., "0006")
    pub data_type: Option<String>,
    /// `@lowLimit`
    pub low_limit: Option<String>,
    /// `@highLimit`
    pub high_limit: Option<String>,
    /// `@accessType`
    pub access_type: Option<ObjectAccessType>,
    /// `@PDOmapping`
    pub pdo_mapping: Option<ObjectPdoMapping>,
    /// `@objFlags`
    pub obj_flags: Option<String>,
    
    // --- Value ---
    /// The resolved data for this sub-object, from `actualValue` or `defaultValue`.
    pub data: Option<Vec<u8>>,
}