// crates/powerlink-rs/src/od/value.rs
// Added support for serializing/deserializing more types relevant to PDO.

use crate::PowerlinkError;
use crate::common::{NetTime, TimeDifference, TimeOfDay};
use crate::frame::basic::MacAddress;
use crate::types::{
    BOOLEAN, INTEGER8, INTEGER16, INTEGER32, INTEGER64, IpAddress, REAL32, REAL64, UNSIGNED8,
    UNSIGNED16, UNSIGNED32, UNSIGNED64,
};
use alloc::{string::String, vec::Vec};
use core::convert::TryInto; // Required for try_into()

/// Represents any value that can be stored in an Object Dictionary entry.
#[derive(Debug, Clone, PartialEq)]
pub enum ObjectValue {
    Boolean(BOOLEAN), // Actually u8
    Integer8(INTEGER8),
    Integer16(INTEGER16),
    Integer32(INTEGER32),
    Integer64(INTEGER64),
    Unsigned8(UNSIGNED8),
    Unsigned16(UNSIGNED16),
    Unsigned32(UNSIGNED32),
    Unsigned64(UNSIGNED64),
    Real32(REAL32),
    Real64(REAL64),
    VisibleString(String),   // Typically limited length
    OctetString(Vec<u8>),    // Typically limited length
    UnicodeString(Vec<u16>), // Typically limited length
    Domain(Vec<u8>),         // Large binary data
    TimeOfDay(TimeOfDay),
    TimeDifference(TimeDifference),
    NetTime(NetTime),
    MacAddress(MacAddress), // Array [u8; 6]
    IpAddress(IpAddress),   // Array [u8; 4]
}

impl ObjectValue {
    /// Serializes the inner value into a little-endian byte vector.
    /// Suitable for PDO payload construction.
    pub fn serialize(&self) -> Vec<u8> {
        match self {
            // Fixed-size numeric types
            ObjectValue::Boolean(v) => v.to_le_bytes().to_vec(), // Serialize as u8
            ObjectValue::Integer8(v) => v.to_le_bytes().to_vec(),
            ObjectValue::Integer16(v) => v.to_le_bytes().to_vec(),
            ObjectValue::Integer32(v) => v.to_le_bytes().to_vec(),
            ObjectValue::Integer64(v) => v.to_le_bytes().to_vec(),
            ObjectValue::Unsigned8(v) => v.to_le_bytes().to_vec(),
            ObjectValue::Unsigned16(v) => v.to_le_bytes().to_vec(),
            ObjectValue::Unsigned32(v) => v.to_le_bytes().to_vec(),
            ObjectValue::Unsigned64(v) => v.to_le_bytes().to_vec(),
            ObjectValue::Real32(v) => v.to_le_bytes().to_vec(),
            ObjectValue::Real64(v) => v.to_le_bytes().to_vec(),

            // Byte arrays / Strings (handle potential length limits elsewhere if needed)
            ObjectValue::VisibleString(v) => v.as_bytes().to_vec(), // ASCII bytes
            ObjectValue::OctetString(v) => v.clone(),
            ObjectValue::Domain(v) => v.clone(), // Often large, clone needed

            // Complex types - serialize components in LE order
            ObjectValue::TimeOfDay(v) => [
                v.ms.to_le_bytes().as_slice(),   // U28 + 4 reserved bits -> U32 LE
                v.days.to_le_bytes().as_slice(), // U16 LE
            ]
            .concat(), // Total 6 bytes
            ObjectValue::TimeDifference(v) => [
                v.ms.to_le_bytes().as_slice(),   // U28 + 4 reserved bits -> U32 LE
                v.days.to_le_bytes().as_slice(), // U16 LE
            ]
            .concat(), // Total 6 bytes
            ObjectValue::NetTime(v) => [
                v.seconds.to_le_bytes().as_slice(),     // U32 LE
                v.nanoseconds.to_le_bytes().as_slice(), // U32 LE
            ]
            .concat(), // Total 8 bytes
            ObjectValue::MacAddress(v) => v.0.to_vec(), // 6 bytes
            ObjectValue::IpAddress(v) => v.to_vec(),    // 4 bytes

            // UnicodeString needs special handling (each u16 to LE bytes)
            ObjectValue::UnicodeString(v) => v.iter().flat_map(|c| c.to_le_bytes()).collect(),
        }
    }

    /// Deserializes a byte slice into a new ObjectValue, using an existing
    /// ObjectValue as a type template. Assumes little-endian data.
    pub fn deserialize(
        data: &[u8],
        type_template: &ObjectValue,
    ) -> Result<ObjectValue, PowerlinkError> {
        // Helper macro to handle fixed-size deserialization
        macro_rules! deserialize_fixed {
            ($data:expr, $variant:path, $type:ty) => {{
                // Check length before trying to convert
                let expected_len = core::mem::size_of::<$type>();
                if $data.len() < expected_len {
                    Err(PowerlinkError::BufferTooShort) // Use BufferTooShort for length issues
                } else {
                    // Use try_into directly on the correctly sized sub-slice
                    match $data[..expected_len].try_into() {
                        Ok(bytes) => Ok($variant(<$type>::from_le_bytes(bytes))),
                        Err(_) => Err(PowerlinkError::SliceConversion), // Should not happen if length check passes
                    }
                }
            }};
        }

        match type_template {
            ObjectValue::Boolean(_) => deserialize_fixed!(data, ObjectValue::Boolean, u8),
            ObjectValue::Integer8(_) => deserialize_fixed!(data, ObjectValue::Integer8, i8),
            ObjectValue::Integer16(_) => deserialize_fixed!(data, ObjectValue::Integer16, i16),
            ObjectValue::Integer32(_) => deserialize_fixed!(data, ObjectValue::Integer32, i32),
            ObjectValue::Integer64(_) => deserialize_fixed!(data, ObjectValue::Integer64, i64),
            ObjectValue::Unsigned8(_) => deserialize_fixed!(data, ObjectValue::Unsigned8, u8),
            ObjectValue::Unsigned16(_) => deserialize_fixed!(data, ObjectValue::Unsigned16, u16),
            ObjectValue::Unsigned32(_) => deserialize_fixed!(data, ObjectValue::Unsigned32, u32),
            ObjectValue::Unsigned64(_) => deserialize_fixed!(data, ObjectValue::Unsigned64, u64),
            ObjectValue::Real32(_) => deserialize_fixed!(data, ObjectValue::Real32, f32),
            ObjectValue::Real64(_) => deserialize_fixed!(data, ObjectValue::Real64, f64),
            ObjectValue::VisibleString(_) => Ok(ObjectValue::VisibleString(
                // Assuming UTF-8 conversion is okay for VisibleString (ASCII subset)
                String::from_utf8(data.to_vec()).map_err(|_| PowerlinkError::TypeMismatch)?,
            )),
            ObjectValue::OctetString(_) => Ok(ObjectValue::OctetString(data.to_vec())),
            ObjectValue::Domain(_) => Ok(ObjectValue::Domain(data.to_vec())),

            // Complex types - deserialize components in LE order
            ObjectValue::TimeOfDay(_) => {
                if data.len() < 6 {
                    Err(PowerlinkError::BufferTooShort)
                } else {
                    Ok(ObjectValue::TimeOfDay(TimeOfDay {
                        ms: u32::from_le_bytes(data[0..4].try_into()?), // U28 + 4 reserved bits
                        days: u16::from_le_bytes(data[4..6].try_into()?),
                    }))
                }
            }
            ObjectValue::TimeDifference(_) => {
                if data.len() < 6 {
                    Err(PowerlinkError::BufferTooShort)
                } else {
                    Ok(ObjectValue::TimeDifference(TimeDifference {
                        ms: u32::from_le_bytes(data[0..4].try_into()?), // U28 + 4 reserved bits
                        days: u16::from_le_bytes(data[4..6].try_into()?),
                    }))
                }
            }
            ObjectValue::NetTime(_) => {
                if data.len() < 8 {
                    Err(PowerlinkError::BufferTooShort)
                } else {
                    Ok(ObjectValue::NetTime(NetTime {
                        seconds: u32::from_le_bytes(data[0..4].try_into()?),
                        nanoseconds: u32::from_le_bytes(data[4..8].try_into()?),
                    }))
                }
            }
            ObjectValue::MacAddress(_) => {
                if data.len() < 6 {
                    Err(PowerlinkError::BufferTooShort)
                } else {
                    Ok(ObjectValue::MacAddress(crate::frame::basic::MacAddress(
                        data[0..6].try_into()?,
                    )))
                }
            }
            ObjectValue::IpAddress(_) => {
                if data.len() < 4 {
                    Err(PowerlinkError::BufferTooShort)
                } else {
                    Ok(ObjectValue::IpAddress(data[0..4].try_into()?))
                }
            }

            // UnicodeString needs special handling (LE bytes pairs to u16)
            ObjectValue::UnicodeString(_) => {
                if data.len() % 2 != 0 {
                    Err(PowerlinkError::TypeMismatch)
                }
                // Must be even length
                else {
                    let chars: Result<Vec<u16>, _> = data
                        .chunks_exact(2)
                        .map(|chunk| chunk.try_into().map(u16::from_le_bytes))
                        .collect();
                    Ok(ObjectValue::UnicodeString(chars?)) // Propagate potential slice conversion error
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::{NetTime, TimeDifference, TimeOfDay};
    use crate::frame::basic::MacAddress;
    use alloc::vec;

    #[test]
    fn test_basic_types_roundtrip() {
        let val_u8 = ObjectValue::Unsigned8(0xAA);
        assert_eq!(
            ObjectValue::deserialize(&val_u8.serialize(), &val_u8),
            Ok(val_u8)
        );

        let val_u16 = ObjectValue::Unsigned16(0xAABB);
        assert_eq!(
            ObjectValue::deserialize(&val_u16.serialize(), &val_u16),
            Ok(val_u16)
        );

        let val_u32 = ObjectValue::Unsigned32(0xAABBCCDD);
        assert_eq!(
            ObjectValue::deserialize(&val_u32.serialize(), &val_u32),
            Ok(val_u32)
        );

        let val_i32 = ObjectValue::Integer32(-123456);
        assert_eq!(
            ObjectValue::deserialize(&val_i32.serialize(), &val_i32),
            Ok(val_i32)
        );
        
        let val_bool = ObjectValue::Boolean(1);
        assert_eq!(
            ObjectValue::deserialize(&val_bool.serialize(), &val_bool),
            Ok(val_bool)
        );
    }

    #[test]
    fn test_string_roundtrip() {
        let original = ObjectValue::VisibleString("Powerlink".into());
        let bytes = original.serialize();
        let deserialized = ObjectValue::deserialize(&bytes, &original).unwrap();
        assert_eq!(original, deserialized);
    }

    #[test]
    fn test_octet_string_roundtrip() {
        let original = ObjectValue::OctetString(vec![0x01, 0x02, 0x03, 0x04]);
        let bytes = original.serialize();
        let deserialized = ObjectValue::deserialize(&bytes, &original).unwrap();
        assert_eq!(original, deserialized);
    }
    
    #[test]
    fn test_unicode_string_roundtrip() {
        let original = ObjectValue::UnicodeString(vec![0x0048, 0x0069]); // "Hi"
        let bytes = original.serialize();
        assert_eq!(bytes.len(), 4);
        let deserialized = ObjectValue::deserialize(&bytes, &original).unwrap();
        assert_eq!(original, deserialized);
    }
    
    #[test]
    fn test_unicode_string_odd_length_error() {
        let template = ObjectValue::UnicodeString(vec![]);
        let odd_bytes = vec![0x00, 0x48, 0x00]; // 3 bytes
        let result = ObjectValue::deserialize(&odd_bytes, &template);
        assert_eq!(result, Err(PowerlinkError::TypeMismatch));
    }

    #[test]
    fn test_complex_types_roundtrip() {
        // MacAddress
        let mac = ObjectValue::MacAddress(MacAddress([0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC]));
        let bytes = mac.serialize();
        assert_eq!(bytes, vec![0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC]);
        assert_eq!(ObjectValue::deserialize(&bytes, &mac), Ok(mac));

        // IpAddress
        let ip = ObjectValue::IpAddress([192, 168, 100, 1]);
        let bytes = ip.serialize();
        assert_eq!(bytes, vec![192, 168, 100, 1]);
        assert_eq!(ObjectValue::deserialize(&bytes, &ip), Ok(ip));

        // NetTime
        let time = ObjectValue::NetTime(NetTime { seconds: 100, nanoseconds: 500 });
        let bytes = time.serialize();
        assert_eq!(bytes.len(), 8);
        assert_eq!(ObjectValue::deserialize(&bytes, &time), Ok(time));
        
        // TimeOfDay
        let tod = ObjectValue::TimeOfDay(TimeOfDay { ms: 123456, days: 5000 });
        let bytes = tod.serialize();
        assert_eq!(bytes.len(), 6);
        assert_eq!(ObjectValue::deserialize(&bytes, &tod), Ok(tod));
    }

    #[test]
    fn test_buffer_too_short() {
        let u32_template = ObjectValue::Unsigned32(0);
        let short_buf = [0xAA, 0xBB, 0xCC]; // 3 bytes
        assert_eq!(
            ObjectValue::deserialize(&short_buf, &u32_template),
            Err(PowerlinkError::BufferTooShort)
        );

        let mac_template = ObjectValue::MacAddress(MacAddress::default());
        let short_mac = [0x00; 5];
        assert_eq!(
            ObjectValue::deserialize(&short_mac, &mac_template),
            Err(PowerlinkError::BufferTooShort)
        );
    }
}