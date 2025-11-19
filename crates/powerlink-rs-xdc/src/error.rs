use alloc::fmt;
use alloc::string::String;
use core::num::ParseIntError;
use hex::FromHexError;
use quick_xml::Error as XmlError;
use quick_xml::errors::serialize::{DeError, SeError};

/// Errors that can occur during XDC parsing, validation, or serialization.
#[derive(Debug)]
pub enum XdcError {
    /// An error occurred while deserializing the XML structure.
    XmlParsing(DeError),

    /// An error occurred while serializing data to XML.
    XmlSerializing(SeError),

    /// An I/O or syntax error occurred in the underlying XML writer.
    XmlWriting(XmlError),

    /// The `actualValue` or `defaultValue` attribute contained invalid hex data.
    HexParsing(FromHexError),

    /// An error occurred during string formatting.
    FmtError(fmt::Error),

    /// A mandatory XML element required by the POWERLINK profile (e.g., `ProfileBody`) was missing.
    MissingElement { element: &'static str },

    /// A mandatory attribute (e.g., `@index`) was missing from an element.
    MissingAttribute { attribute: &'static str },

    /// An attribute (e.g., `@index`) existed but had an invalid format (e.g., non-hex string).
    InvalidAttributeFormat { attribute: &'static str },

    /// A generic validation error occurred during profile resolution.
    ValidationError(&'static str),

    /// The parsed data length does not match the definition provided in `dataType`.
    TypeValidationError {
        index: u16,
        sub_index: u8,
        data_type: String,
        expected_bytes: usize,
        actual_bytes: usize,
    },

    /// The requested functionality is not yet implemented.
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

/// Converts `ParseIntError` (typically from reading hex index/subindex) into a domain-specific error.
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
    use crate::model;
    use alloc::string::ToString;
    use hex;
    use quick_xml;

    #[test]
    fn test_from_de_error() {
        let xml_err =
            quick_xml::de::from_str::<model::header::ProfileHeader>("<Test></Test>").unwrap_err();
        let xdc_err: XdcError = xml_err.into();
        assert!(matches!(xdc_err, XdcError::XmlParsing(_)));
    }

    #[test]
    fn test_from_se_error() {
        let xml_err = quick_xml::errors::serialize::SeError::Custom("test error".to_string());
        let xdc_err: XdcError = xml_err.into();
        assert!(matches!(xdc_err, XdcError::XmlSerializing(_)));
    }

    #[test]
    fn test_from_xml_error() {
        let xml_err = quick_xml::Error::Syntax(quick_xml::errors::SyntaxError::InvalidBangMarkup);
        let xdc_err: XdcError = xml_err.into();
        assert!(matches!(xdc_err, XdcError::XmlWriting(_)));
    }

    #[test]
    fn test_from_hex_error() {
        let hex_err = hex::decode("Z").unwrap_err();
        let xdc_err: XdcError = hex_err.into();
        assert!(matches!(xdc_err, XdcError::HexParsing(_)));
    }

    #[test]
    fn test_from_fmt_error() {
        let fmt_err = core::fmt::Error;
        let xdc_err: XdcError = fmt_err.into();
        assert!(matches!(xdc_err, XdcError::FmtError(_)));
    }

    #[test]
    fn test_from_parse_int_error() {
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
