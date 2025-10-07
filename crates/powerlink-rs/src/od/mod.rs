use crate::common::{NetTime, TimeDifference, TimeOfDay};
use crate::types::{
    BOOLEAN, INTEGER16, INTEGER32, INTEGER64, INTEGER8, REAL32, REAL64, UNSIGNED16,
    UNSIGNED32, UNSIGNED64, UNSIGNED8, IpAddress, MacAddress,
};
use alloc::{
    borrow::Cow,
    collections::BTreeMap,
    string::String,
    vec::Vec,
};

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
    ReadOnly,
    WriteOnly,
    WriteOnlYStore,
    ReadWrite,
    ReadWriteStore,
    Constant,
    Conditional,
}

/// A complete entry in the Object Dictionary, containing both the data and its metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectEntry {
    /// The actual data, stored in the existing Object enum.
    pub object: Object,
    /// A descriptive name for the object (for diagnostics or future SDO-by-name).
    pub name: &'static str,
    /// The access rights for this object (e.g., ReadOnly).
    pub access: AccessType,
}

/// The main Object Dictionary structure.
#[derive(Debug, Default)]
pub struct ObjectDictionary {
    entries: BTreeMap<u16, ObjectEntry>,
}

impl ObjectDictionary {
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts a new object entry into the dictionary at a given index.
    pub fn insert(&mut self, index: u16, entry: ObjectEntry) {
        self.entries.insert(index, entry);
    }

    /// Reads a value from the Object Dictionary by index and sub-index.
    ///
    /// This function returns a `Cow<ObjectValue>` to efficiently handle two cases:
    /// - For normal data access (`sub_index > 0`), it returns a cheap `Cow::Borrowed` reference.
    /// - For `sub_index == 0` on complex types, it returns a temporary `Cow::Owned` value
    ///   representing the number of entries, as required by the specification.    
    pub fn read<'a>(&'a self, index: u16, sub_index: u8) -> Option<Cow<'a, ObjectValue>> {
        // UPDATED: Now accesses the `object` field of the `ObjectEntry`.
        self.entries.get(&index).and_then(|entry| match &entry.object {
            Object::Variable(value) => {
                if sub_index == 0 {
                    Some(Cow::Borrowed(value))
                } else {
                    None
                }
            }
            Object::Array(values) | Object::Record(values) => {
                if sub_index == 0 {
                    Some(Cow::Owned(ObjectValue::Unsigned8(values.len() as u8)))
                } else {
                    values.get(sub_index as usize - 1).map(Cow::Borrowed)
                }
            }
        })
    }

    /// Writes a value to the Object Dictionary, respecting the object's access rights.
    pub fn write(&mut self, index: u16, sub_index: u8, value: ObjectValue) -> Result<(), &'static str> {
        self.entries.get_mut(&index).map_or_else(
            || Err("Object index not found"),
            |entry| {
                // First, check access rights. If read-only, return an error from the closure.
                if matches!(entry.access, AccessType::ReadOnly | AccessType::Constant) {
                    return Err("Object is read-only");
                }

                // If access is permitted, proceed with the modification logic.
                match &mut entry.object {
                    Object::Variable(v) => {
                        if sub_index == 0 {
                            // Type checking should be added here in a real implementation
                            *v = value;
                            Ok(())
                        } else {
                            Err("Invalid sub-index for a variable")
                        }
                    }
                    Object::Array(values) | Object::Record(values) => {
                        if sub_index == 0 {
                            Err("Cannot write to sub-index 0")
                        } else if let Some(v) = values.get_mut(sub_index as usize - 1) {
                             // Type checking should be added here
                            *v = value;
                            Ok(())
                        } else {
                            Err("Sub-index out of bounds")
                        }
                    }
                }
            },
        )
    }
}


// --- Unit Tests ---
#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use alloc::borrow::Cow;

    #[test]
    fn test_read_variable() {
        let mut od = ObjectDictionary::new();
        od.insert(0x1006, ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned32(12345)),
            name: "TestVar",
            access: AccessType::ReadWrite,
        });

        let value = od.read(0x1006, 0).unwrap();
        assert_eq!(*value, ObjectValue::Unsigned32(12345));
        // Verify it's a borrowed reference
        assert!(matches!(value, Cow::Borrowed(_)));
    }
    
    #[test]
    fn test_read_array_element() {
        let mut od = ObjectDictionary::new();
        let arr = ObjectEntry { 
            object: Object::Array(vec![ObjectValue::Unsigned16(100)]), 
            name: "TestArray", 
            access: AccessType::ReadWrite
        };
        od.insert(0x2000, arr);

        let value = od.read(0x2000, 1).unwrap();
        assert_eq!(*value, ObjectValue::Unsigned16(100));
        assert!(matches!(value, Cow::Borrowed(_)));
    }

    #[test]
    fn test_read_sub_index_zero_returns_owned_length() {
        let mut od = ObjectDictionary::new();
        let arr = ObjectEntry { 
            object: Object::Array(vec![
                ObjectValue::Unsigned16(100), 
                ObjectValue::Unsigned16(200)
            ]), 
            name: "TestArray", 
            access: AccessType::ReadWrite
        };
        od.insert(0x2000, arr);

        let value = od.read(0x2000, 0).unwrap();
        // UPDATED: Dereference the Cow for comparison.
        assert_eq!(*value, ObjectValue::Unsigned8(2));
        assert!(matches!(value, Cow::Owned(_)));
    }

    #[test]
    fn test_create_and_insert_variable() {
        let mut od = ObjectDictionary::new();
        let obj = ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned32(12345)),
            name: "TestVar",
            access: AccessType::ReadWrite,
        };
        od.insert(0x1006, obj);

        let value = od.read(0x1006, 0).unwrap();
        assert_eq!(*value, ObjectValue::Unsigned32(12345));
    }

    #[test]
    fn test_read_write_variable() {
        let mut od = ObjectDictionary::new();
        od.insert(0x1008, ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned8(10)),
            name: "TestVar",
            access: AccessType::ReadWrite,
        });

        assert_eq!(*od.read(0x1008, 0).unwrap(), ObjectValue::Unsigned8(10));
        od.write(0x1008, 0, ObjectValue::Unsigned8(42)).unwrap();
        assert_eq!(*od.read(0x1008, 0).unwrap(), ObjectValue::Unsigned8(42));
    }

    #[test]
    fn test_read_write_array() {
        let mut od = ObjectDictionary::new();
        let arr = ObjectEntry { 
            object: Object::Array(vec![
                ObjectValue::Unsigned16(100), 
                ObjectValue::Unsigned16(200)
            ]), 
            name: "TestArray", 
            access: AccessType::ReadWrite
        };
        od.insert(0x2000, arr);

        // Read initial values
        // UPDATED: Dereference the Cow for comparison.
        assert_eq!(*od.read(0x2000, 1).unwrap(), ObjectValue::Unsigned16(100));
        assert_eq!(*od.read(0x2000, 2).unwrap(), ObjectValue::Unsigned16(200));

        // Write to the second element
        od.write(0x2000, 2, ObjectValue::Unsigned16(999)).unwrap();

        // Verify the change
        assert_eq!(*od.read(0x2000, 2).unwrap(), ObjectValue::Unsigned16(999));
    }

    #[test]
    fn test_read_write_record() {
        let mut od = ObjectDictionary::new();
        let rec = ObjectEntry { 
            object: Object::Record(vec![
                ObjectValue::Unsigned8(1), 
                ObjectValue::Unsigned32(50000)
            ]), 
            name: "TestArray", 
            access: AccessType::ReadWrite
        };
        od.insert(0x1F89, rec);

        // Read initial values
        assert_eq!(*od.read(0x1F89, 1).unwrap(), ObjectValue::Unsigned8(1));
        assert_eq!(*od.read(0x1F89, 2).unwrap(), ObjectValue::Unsigned32(50000));

        // Write to the first element
        od.write(0x1F89, 1, ObjectValue::Unsigned8(255)).unwrap();
        
        // Verify the change
        assert_eq!(*od.read(0x1F89, 1).unwrap(), ObjectValue::Unsigned8(255));
    }

    #[test]
    fn test_error_on_invalid_access() {
        let mut od = ObjectDictionary::new();
        let obj = ObjectEntry {
            object: Object::Variable(ObjectValue::Boolean(1)),
            name: "TestVar",
            access: AccessType::ReadWrite,
        };
        od.insert(0x1000, obj);
        
        // Error: Index does not exist
        assert!(od.read(0x9999, 0).is_none());
        assert!(od.write(0x9999, 0, ObjectValue::Boolean(0)).is_err());

        // Error: Invalid sub-index for a variable
        assert!(od.read(0x1000, 1).is_none());
        assert!(od.write(0x1000, 1, ObjectValue::Boolean(0)).is_err());
    }

    #[test]
    fn test_write_to_readonly_fails() {
        let mut od = ObjectDictionary::new();
        od.insert(0x1008, ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned8(10)),
            name: "ReadOnlyVar",
            access: AccessType::ReadOnly,
        });

        // Attempting to write should return an error.
        let result = od.write(0x1008, 0, ObjectValue::Unsigned8(42));
        assert_eq!(result, Err("Object is read-only"));

        // Verify the original value was not changed.
        assert_eq!(*od.read(0x1008, 0).unwrap(), ObjectValue::Unsigned8(10));
    }

    #[test]
    fn test_read_sub_index_zero_for_array() {
        let mut od = ObjectDictionary::new();
        od.insert(0x2000, ObjectEntry {
            object: Object::Array(vec![
                ObjectValue::Unsigned16(100),
                ObjectValue::Unsigned16(200),
            ]),
            name: "TestArray",
            access: AccessType::ReadWrite,
        });

        let value = od.read(0x2000, 0).unwrap();
        assert_eq!(*value, ObjectValue::Unsigned8(2));
        assert!(matches!(value, Cow::Owned(_)));
    }
}