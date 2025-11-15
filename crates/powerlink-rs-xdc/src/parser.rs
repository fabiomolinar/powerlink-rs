// crates/powerlink-rs-xdc/src/parser.rs

//! The internal XML parser and helper functions for parsing hex strings.

use crate::error::XdcError;
use crate::model;
use crate::resolver; // This module's functions are now called by `load_...`
use crate::resolver::ValueMode; // Import the new ValueMode enum
use crate::types::XdcFile;
use alloc::string::String;
use alloc::vec::Vec;
use core::num::ParseIntError;
use hex::FromHexError;
use quick_xml::de::from_str;

// --- Public API Functions ---

/// Loads XDC data (using `actualValue`) from an XML string.
///
/// This function parses the XML and resolves the data model by prioritizing
/// the `actualValue` attributes, which is standard for XDC (Configuration) files.
pub fn load_xdc_from_str(s: &str) -> Result<XdcFile, XdcError> {
    let container = parse_xml_str(s)?;
    // Call the resolver with ValueMode::Actual
    resolver::resolve_data(container, ValueMode::Actual)
}

/// Loads XDD default data (using `defaultValue`) from an XML string.
///
/// This function parses the XML and resolves the data model by prioritizing
/// the `defaultValue` attributes, which is standard for XDD (Device Description) files.
pub fn load_xdd_defaults_from_str(s: &str) -> Result<XdcFile, XdcError> {
    let container = parse_xml_str(s)?;
    // Call the resolver with ValueMode::Default
    resolver::resolve_data(container, ValueMode::Default)
}

// --- Internal XML Deserialization ---

/// The core internal function that uses `quick-xml` to deserialize the string
/// into the raw `model` structs.
pub(crate) fn parse_xml_str(s: &str) -> Result<model::Iso15745ProfileContainer, XdcError> {
    // quick-xml's deserializer is very efficient.
    // It maps the XML structure directly to our `model` structs.
    from_str(s).map_err(XdcError::from)
}

// --- Hex String Parsing Helpers ---
// These are used by the resolver.

/// Parses a "0x..." or "..." hex string into a `u32`.
pub(crate) fn parse_hex_u32(s: &str) -> Result<u32, ParseIntError> {
    let s_no_prefix = s.strip_prefix("0x").unwrap_or(s);
    u32::from_str_radix(s_no_prefix, 16)
}

/// Parses a "0x..." or "..." hex string into a `u16`.
pub(crate) fn parse_hex_u16(s: &str) -> Result<u16, ParseIntError> {
    let s_no_prefix = s.strip_prefix("0x").unwrap_or(s);
    u16::from_str_radix(s_no_prefix, 16)
}

/// Parses a "0x..." or "..." hex string into a `u8`.
pub(crate) fn parse_hex_u8(s: &str) -> Result<u8, ParseIntError> {
    let s_no_prefix = s.strip_prefix("0x").unwrap_or(s);
    u8::from_str_radix(s_no_prefix, 16)
}

/// Parses a "0x..." or "..." hex string into a byte vector.
pub(crate) fn parse_hex_string(s: &str) -> Result<Vec<u8>, FromHexError> {
    let s_no_prefix = s.strip_prefix("0x").unwrap_or(s);
    
    // Handle odd-length strings by padding with a leading zero
    if s_no_prefix.len() % 2 != 0 {
        let mut padded_s = String::with_capacity(s_no_prefix.len() + 1);
        padded_s.push('0');
        padded_s.push_str(s_no_prefix);
        hex::decode(padded_s)
    } else {
        hex::decode(s_no_prefix)
    }
}