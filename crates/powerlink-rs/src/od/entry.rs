use super::value::ObjectValue;
use alloc::vec::Vec;

/// Represents a single entry in the Object Dictionary.
#[derive(Debug, Clone, PartialEq)]
pub enum Object {
    Variable(ObjectValue),
    Array(Vec<ObjectValue>),
    Record(Vec<ObjectValue>),
}

/// Defines the access rights for an Object Dictionary entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessType {
    /// read only access
    ReadOnly,
    /// write only access
    WriteOnly,
    /// write only access, value shall be stored
    WriteOnlyStore,
    /// read and write access
    ReadWrite,
    /// read and write access, value shall be stored
    ReadWriteStore,
    /// read only access, value is constant
    Constant,
    /// variable access controlled by the device
    Conditional,
}

/// Defines if an object is mandatory or optional.
/// (Reference: EPSG DS 301, Section 6.2.1)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Mandatory,
    Optional,
    Conditional,
}

/// Defines the PDO mapping options for an object.
/// (Reference: EPSG DS 301, Table 39)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PdoMapping {
    No,
    Optional,
    Default,
}

/// Represents a range of valid values for an object.
#[derive(Debug, Clone, PartialEq)]
pub struct ValueRange {
    pub min: ObjectValue,
    pub max: ObjectValue,
}

/// A complete entry in the Object Dictionary, containing both the data and its metadata.
/// (Reference: EPSG DS 301, Section 6.2.1)
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectEntry {
    /// The actual data, stored in the existing Object enum.
    pub object: Object,
    /// A descriptive name for the object.
    pub name: &'static str,
    /// The category of the object (Mandatory, Optional, etc.).
    pub category: Category,
    /// The access rights for this object. `None` for complex types.
    pub access: Option<AccessType>,
    /// The default value for this object. `None` for complex types.
    pub default_value: Option<ObjectValue>,
    /// The valid value range for this object. `None` for complex types.
    pub value_range: Option<ValueRange>,
    /// The PDO mapping possibility for this object. `None` for complex types.
    pub pdo_mapping: Option<PdoMapping>,
}