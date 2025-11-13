// src/types.rs

use alloc::string::String;
use alloc::vec::Vec;

/// Represents the contents of a parsed XDC or XDD file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct XdcFile {
    /// Device identity information (Vendor, Product, etc.).
    pub identity: Identity,
    /// List of Object Dictionary entries.
    pub data: CfmData,
}

/// Represents the device's identity, parsed from the Device Profile.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Identity {
    pub vendor_id: u32,
    pub product_id: u32,
    pub vendor_name: Option<String>,
    pub product_name: Option<String>,
    pub versions: Vec<Version>,
}

/// A hardware, software, or firmware version entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub version_type: String,
    pub value: String,
}

/// Represents the clean, binary-ready configuration data extracted from an XDC.
///
/// This struct holds the data for Object Dictionary entries.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CfmData {
    /// A list of all parsed objects.
    pub objects: Vec<CfmObject>,
}

/// A single, binary-ready Object Dictionary entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CfmObject {
    /// The Object Dictionary index (e.g., 0x1F22).
    pub index: u16,
    /// The Object Dictionary sub-index (e.g., 0x01).
    pub sub_index: u8,
    /// The raw binary data from the `actualValue` or `defaultValue` attribute.
    pub data: Vec<u8>,
}