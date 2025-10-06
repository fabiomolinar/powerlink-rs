use crate::types::{
    BOOLEAN, INTEGER16, INTEGER32, INTEGER8, UNSIGNED16, UNSIGNED32, UNSIGNED8,
};
use alloc::{collections::BTreeMap, vec::Vec};

/// Represents any value that can be stored in an Object Dictionary entry.
/// This enum covers the basic data types from the specification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectValue {
    Boolean(BOOLEAN),
    Integer8(INTEGER8),
    Integer16(INTEGER16),
    Integer32(INTEGER32),
    Unsigned8(UNSIGNED8),
    Unsigned16(UNSIGNED16),
    Unsigned32(UNSIGNED32),
    // Other types like strings, domains, etc., would be added here.
}

/// Represents a single entry in the Object Dictionary.
/// It can be a simple variable or a complex data structure like an array or record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Object {
    /// A single value.
    Variable(ObjectValue),
    /// A collection of sub-entries, all of the same type.
    Array(Vec<ObjectValue>),
    /// A collection of sub-entries, potentially of different types.
    Record(Vec<ObjectValue>),
}

/// The main Object Dictionary structure.
///
/// Internally, it uses a BTreeMap to store objects, mapping a 16-bit index
/// to an `Object` enum. This is efficient and works in `no_std` environments.
#[derive(Debug, Default)]
pub struct ObjectDictionary {
    entries: BTreeMap<u16, Object>,
}

impl ObjectDictionary {
    /// Creates a new, empty Object Dictionary.
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts a new object into the dictionary at a given index.
    pub fn insert(&mut self, index: u16, object: Object) {
        self.entries.insert(index, object);
    }

    /// Reads a value from the Object Dictionary by index and sub-index.
    pub fn read(&self, index: u16, sub_index: u8) -> Option<&ObjectValue> {
        self.entries.get(&index).and_then(|object| match object {
            Object::Variable(value) => {
                if sub_index == 0 {
                    Some(value)
                } else {
                    None // Variables only have a sub-index of 0.
                }
            }
            Object::Array(values) | Object::Record(values) => {
                if sub_index == 0 {
                    // Sub-index 0 of a complex type holds the number of entries.
                    // For now, we return None, but this would be implemented.
                    None
                } else {
                    // Sub-indices are 1-based for data access.
                    values.get(sub_index as usize - 1)
                }
            }
        })
    }

    /// Writes a value to the Object Dictionary by index and sub-index.
    pub fn write(&mut self, index: u16, sub_index: u8, value: ObjectValue) -> Result<(), &'static str> {
        self.entries.get_mut(&index).map_or_else(
            || Err("Object index not found"),
            |object| match object {
                Object::Variable(v) => {
                    if sub_index == 0 {
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
                        *v = value;
                        Ok(())
                    } else {
                        Err("Sub-index out of bounds")
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


    #[test]
    fn test_create_and_insert_variable() {
        let mut od = ObjectDictionary::new();
        let obj = Object::Variable(ObjectValue::Unsigned32(12345));
        od.insert(0x1006, obj);

        let value = od.read(0x1006, 0).unwrap();
        assert_eq!(*value, ObjectValue::Unsigned32(12345));
    }

    #[test]
    fn test_read_write_variable() {
        let mut od = ObjectDictionary::new();
        od.insert(0x1008, Object::Variable(ObjectValue::Unsigned8(10)));

        // Read initial value
        assert_eq!(*od.read(0x1008, 0).unwrap(), ObjectValue::Unsigned8(10));

        // Write a new value
        od.write(0x1008, 0, ObjectValue::Unsigned8(42)).unwrap();

        // Read the new value
        assert_eq!(*od.read(0x1008, 0).unwrap(), ObjectValue::Unsigned8(42));
    }

    #[test]
    fn test_read_write_array() {
        let mut od = ObjectDictionary::new();
        let arr = Object::Array(vec![
            ObjectValue::Unsigned16(100),
            ObjectValue::Unsigned16(200),
        ]);
        od.insert(0x2000, arr);

        // Read initial values
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
        let rec = Object::Record(vec![
            ObjectValue::Unsigned8(1),
            ObjectValue::Unsigned32(50000),
        ]);
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
        od.insert(0x1000, Object::Variable(ObjectValue::Boolean(1)));
        
        // Error: Index does not exist
        assert!(od.read(0x9999, 0).is_none());
        assert!(od.write(0x9999, 0, ObjectValue::Boolean(0)).is_err());

        // Error: Invalid sub-index for a variable
        assert!(od.read(0x1000, 1).is_none());
        assert!(od.write(0x1000, 1, ObjectValue::Boolean(0)).is_err());
    }
}