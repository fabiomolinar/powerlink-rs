// crates/powerlink-rs/src/od/value.rs

use crate::common::{NetTime, TimeDifference, TimeOfDay};
use crate::types::{
    BOOLEAN, INTEGER16, INTEGER32, INTEGER64, INTEGER8, REAL32, REAL64, UNSIGNED16, UNSIGNED32,
    UNSIGNED64, UNSIGNED8, IpAddress, MacAddress,
};
use crate::PowerlinkError;
use alloc::{string::String, vec, vec::Vec};

/// Represents any value that can be stored in an Object Dictionary entry.
#[derive(Debug, Clone, PartialEq)]
pub enum ObjectValue {
    Boolean(BOOLEAN),
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
    VisibleString(String),
    OctetString(Vec<u8>),
    UnicodeString(Vec<u16>),
    Domain(Vec<u8>),
    TimeOfDay(TimeOfDay),
    TimeDifference(TimeDifference),
    NetTime(NetTime),
    MacAddress(MacAddress),
    IpAddress(IpAddress),
}

impl ObjectValue {
    /// Serializes the inner value into a little-endian byte vector.
    pub fn serialize(&self) -> Vec<u8> {
        match self {
            ObjectValue::Boolean(v) => v.to_le_bytes().to_vec(),
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
            ObjectValue::VisibleString(v) => v.as_bytes().to_vec(),
            ObjectValue::OctetString(v) => v.clone(),
            ObjectValue::Domain(v) => v.clone(),
            // Other types would be serialized here.
            _ => vec![],
        }
    }

    /// Deserializes a byte slice into a new ObjectValue, using an existing
    /// ObjectValue as a type template.
    pub fn deserialize(
        data: &[u8],
        type_template: &ObjectValue,
    ) -> Result<ObjectValue, PowerlinkError> {
        // Helper macro to handle fixed-size deserialization
        macro_rules! deserialize_fixed {
            ($data:expr, $variant:path, $type:ty) => {
                $data
                    .try_into()
                    .map(|bytes| $variant(<$type>::from_le_bytes(bytes)))
                    .map_err(|_| PowerlinkError::TypeMismatch)
            };
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
                String::from_utf8(data.to_vec()).map_err(|_| PowerlinkError::TypeMismatch)?,
            )),
            ObjectValue::OctetString(_) => Ok(ObjectValue::OctetString(data.to_vec())),
            ObjectValue::Domain(_) => Ok(ObjectValue::Domain(data.to_vec())),
            // Other types would be deserialized here.
            _ => Err(PowerlinkError::TypeMismatch),
        }
    }
}