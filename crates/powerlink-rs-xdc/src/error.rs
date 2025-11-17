// crates/powerlink-rs-xdc/src/error.rs

use alloc::fmt;
use alloc::string::String;
use core::num::ParseIntError;
use hex::FromHexError;
use quick_xml::Error as XmlError;
use quick_xml::errors::serialize::DeError;
use quick_xml::errors::serialize::SeError;

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

    /// A generic validation error.
    ValidationError(&'static str),

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

#[cfg(test)]
mod tests {
    use super::XdcError;
    use crate::model; // Import model for test
    use alloc::string::ToString;
    use hex;
    use quick_xml;

    #[test]
    fn test_from_de_error() {
        // Create a dummy DeError by failing to parse a struct with required fields
        let xml_err =
            quick_xml::de::from_str::<model::header::ProfileHeader>("<Test></Test>").unwrap_err();
        let xdc_err: XdcError = xml_err.into();
        assert!(matches!(xdc_err, XdcError::XmlParsing(_)));
    }

    #[test]
    fn test_from_se_error() {
        // Create a dummy SeError
        let xml_err = quick_xml::errors::serialize::SeError::Custom("test error".to_string());
        let xdc_err: XdcError = xml_err.into();
        assert!(matches!(xdc_err, XdcError::XmlSerializing(_)));
    }

    #[test]
    fn test_from_xml_error() {
        // Create a dummy XmlError
        let xml_err = quick_xml::Error::Syntax(quick_xml::errors::SyntaxError::InvalidBangMarkup);
        let xdc_err: XdcError = xml_err.into();
        assert!(matches!(xdc_err, XdcError::XmlWriting(_)));
    }

    #[test]
    fn test_from_hex_error() {
        // Create a dummy FromHexError by parsing invalid hex
        let hex_err = hex::decode("Z").unwrap_err();
        let xdc_err: XdcError = hex_err.into();
        assert!(matches!(xdc_err, XdcError::HexParsing(_)));
    }

    #[test]
    fn test_from_fmt_error() {
        // Create a dummy fmt::Error
        let fmt_err = core::fmt::Error;
        let xdc_err: XdcError = fmt_err.into();
        assert!(matches!(xdc_err, XdcError::FmtError(_)));
    }

    #[test]
    fn test_from_parse_int_error() {
        // Create a dummy ParseIntError
        let parse_err = "not a number".parse::<u16>().unwrap_err();
        let xdc_err: XdcError = parse_err.into();
        assert!(matches!(
            xdc_err,
            XdcError::InvalidAttributeFormat {
                attribute: "index or subIndex"
            }
        ));
    }
}
