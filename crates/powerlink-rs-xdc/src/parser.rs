// crates/powerlink-rs-xdc/src/parser.rs

use crate::error::XdcError;
use crate::model::{self, SubObject};
use crate::resolver; // <-- NEW: Import the resolver
use crate::types::{XdcFile};
use alloc::string::String;
use alloc::vec::Vec;
use core::num::ParseIntError;

/// Parses an XDC (XML Device Configuration) string slice and extracts CFM object data
/// from `actualValue` attributes.
///
/// This function is used to load a device's final configuration.
///
/// # Arguments
/// * `xml_content` - A string slice containing the full XDC XML file.
///
/// # Errors
/// Returns an `XdcError` if parsing fails, hex conversion fails, or
/// critical elements are missing.
pub fn load_xdc_from_str(xml_content: &str) -> Result<XdcFile, XdcError> {
    // For XDC (configuration), we only care about `actualValue`.
    // We do not resolve `uniqueIDRef` as `actualValue` is required to be present.
    load_from_str_internal(xml_content, |so| so.actual_value.as_ref())
}

/// Parses an XDD (XML Device Description) string slice and extracts CFM object data
/// from `defaultValue` attributes.
///
/// This function is used to load the default factory configuration from
/// a device description file. It supports resolving `uniqueIDRef` attributes
/// to the `ApplicationProcess` parameter list.
///
/// # Errors
/// Returns an `XdcError` if parsing fails, hex conversion fails, or
/// critical elements are missing.
pub fn load_xdd_defaults_from_str(xml_content: &str) -> Result<XdcFile, XdcError> {
    // For XDD (description), we prioritize `defaultValue` but will
    // fall back to resolving `uniqueIDRef` if it's not present.
    load_from_str_internal(xml_content, |so| so.default_value.as_ref())
}

/// Internal parsing logic that accepts a closure to select the correct
/// attribute (`actualValue` or `defaultValue`).
fn load_from_str_internal(
    xml_content: &str,
    value_selector: impl Fn(&SubObject) -> Option<&String>,
) -> Result<XdcFile, XdcError> {
    // 1. Deserialize the raw XML string into our internal model.
    let container: model::Iso15745ProfileContainer = quick_xml::de::from_str(xml_content)?;

    // 2. Pass the deserialized struct to the resolver for logic processing.
    //    All complex logic (parameter mapping, value resolution, type validation)
    //    is now handled by the resolver.
    resolver::resolve_data(container, value_selector)
}

// --- Helper Functions (Public for use in builder.rs and resolver.rs) ---

/// Parses a "0x..." or "..." hex string into a u32.
pub fn parse_hex_u32(s: &str) -> Result<u32, ParseIntError> {
    let trimmed = s.strip_prefix("0x").unwrap_or(s);
    u32::from_str_radix(trimmed, 16)
}

/// Parses a "0x..." or "..." hex string into a u16.
pub fn parse_hex_u16(s: &str) -> Result<u16, ParseIntError> {
    let trimmed = s.strip_prefix("0x").unwrap_or(s);
    u16::from_str_radix(trimmed, 16)
}

/// Parses a "0x..." or "..." hex string into a u8.
pub fn parse_hex_u8(s: &str) -> Result<u8, ParseIntError> {
    let trimmed = s.strip_prefix("0x").unwrap_or(s);
    u8::from_str_radix(trimmed, 16)
}

/// Parses a "0x..." or "..." hex string into a Vec<u8>.
pub fn parse_hex_string(s: &str) -> Result<Vec<u8>, XdcError> {
    let trimmed = s.strip_prefix("0x").unwrap_or(s);
    if trimmed.len() % 2 != 0 {
        return Err(XdcError::HexParsing(hex::FromHexError::OddLength));
    }
    hex::decode(trimmed).map_err(XdcError::HexParsing)
}