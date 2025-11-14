// crates/powerlink-rs-xdc/src/error.rs

use alloc::fmt;
use alloc::string::String;
use core::num::ParseIntError;
use hex::FromHexError;
use quick_xml::errors::serialize::DeError;
use quick_xml::errors::serialize::SeError;
use quick_xml::Error as XmlError;

/// Errors that can occur during XDC parsing or serialization.
#[derive(Debug)]
pub enum XdcError {
    /// An error from the underlying `quick-xml` deserializer.
    XmlParsing(DeError),

    /// An error from the underlying `quick-xml` serializer.
    XmlSerializing(SeError),

    /// An error from the underlying `quick-xml` writer (e.g., I/O).
    XmlWriting(XmlError),

    /// The `actualValue` or `defaultValue` attribute contained invalid hex.
    HexParsing(FromHexError),

    /// An error occurred during string formatting (e.g., in helpers).
    FmtError(fmt::Error),

    /// A required XML element was missing (e.g., ProfileBody).
    MissingElement { element: &'static str },

    /// A required attribute was missing (e.g., @index).
    MissingAttribute { attribute: &'static str },

    /// An attribute (e.g., @index) had an invalid format.
    InvalidAttributeFormat { attribute: &'static str },

    /// The parsed data length does not match the `dataType` attribute.
    TypeValidationError {
        index: u16,
        sub_index: u8,
        data_type: String,
        expected_bytes: usize,
        actual_bytes: usize,
    },

    /// Functionality is not yet implemented.
    NotImplemented,
}

impl From<DeError> for XdcError {
    fn from(e: DeError) -> Self {
        XdcError::XmlParsing(e)
    }
}

impl From<SeError> for XdcError {
    fn from(e: SeError) -> Self {
        XdcError::XmlSerializing(e)
    }
}

impl From<XmlError> for XdcError {
    fn from(e: XmlError) -> Self {
        XdcError::XmlWriting(e)
    }
}

impl From<FromHexError> for XdcError {
    fn from(e: FromHexError) -> Self {
        XdcError::HexParsing(e)
    }
}

impl From<fmt::Error> for XdcError {
    fn from(e: fmt::Error) -> Self {
        XdcError::FmtError(e)
    }
}

/// Converts `ParseIntError` (typically from reading hex index/subindex) into a user-friendly error.
impl From<ParseIntError> for XdcError {
    fn from(_: ParseIntError) -> Self {
        XdcError::InvalidAttributeFormat {
            attribute: "index or subIndex",
        }
    }
}