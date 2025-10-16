use crate::common::{NetTime, TimeDifference, TimeOfDay};
use crate::hal::ObjectDictionaryStorage;
use crate::types::{
    BOOLEAN, INTEGER16, INTEGER32, INTEGER64, INTEGER8, REAL32, REAL64, UNSIGNED16,
    UNSIGNED32, UNSIGNED64, UNSIGNED8, IpAddress, MacAddress,
};
use alloc::{borrow::Cow, collections::BTreeMap, string::String, vec, vec::Vec};
use core::fmt;

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
    /// read only access
    ReadOnly,
    /// write only access
    WriteOnly,
    /// write only access, value shall be stored
    WriteOnlYStore,
    /// read and write access
    ReadWrite,
    /// read and write access, value shall be stored
    ReadWriteStore,
    /// read only access, value is constant
    Constant,
    /// variable access controlled by the device
    Conditional,
}

/// A complete entry in the Object Dictionary, containing both the data and its metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectEntry {
    /// The actual data, stored in the existing Object enum.
    pub object: Object,
    /// A descriptive name for the object.
    pub name: &'static str,
    /// The access rights for this object.
    pub access: AccessType,
}

/// The main Object Dictionary structure.
pub struct ObjectDictionary<'a> {
    entries: BTreeMap<u16, ObjectEntry>,
    storage: Option<&'a mut dyn ObjectDictionaryStorage>,
}

impl<'a> fmt::Debug for ObjectDictionary<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ObjectDictionary")
            .field("entries", &self.entries)
            .field(
                "storage",
                &if self.storage.is_some() {
                    "Some(<Storage Backend>)"
                } else {
                    "None"
                },
            )
            .finish()
    }
}

impl<'a> ObjectDictionary<'a> {
    /// Creates a new OD, populates it with protocol-level objects,
    /// and then loads any persistent values from storage.
    pub fn new(storage: Option<&'a mut dyn ObjectDictionaryStorage>) -> Self {
        let mut od = Self {
            entries: BTreeMap::new(),
            storage,
        };
        od.populate_protocol_objects();
        if let Some(s) = &mut od.storage {
            if let Ok(stored_params) = s.load() {
                for ((index, sub_index), value) in stored_params {
                    // This internal write bypasses access checks during initialization.
                    let _ = od.write_internal(index, sub_index, value, false);
                }
            }
        }
        od
    }

    /// Populates the OD with mandatory objects that define protocol mechanisms.
    /// Device-specific identification objects are left to the user to insert.
    fn populate_protocol_objects(&mut self) {
        // Add Store Parameters (1010h)
        self.insert(0x1010, ObjectEntry {
            object: Object::Record(vec![ObjectValue::Unsigned32(1)]),
            name: "NMT_StoreParam_REC",
            access: AccessType::ReadWrite,
        });

        // Add Restore Default Parameters (1011h)
        self.insert(0x1011, ObjectEntry {
            object: Object::Record(vec![ObjectValue::Unsigned32(1)]),
            name: "NMT_RestoreDefParam_REC",
            access: AccessType::ReadWrite,
        });
    }

    /// Inserts a new object entry into the dictionary at a given index.
    pub fn insert(&mut self, index: u16, entry: ObjectEntry) {
        self.entries.insert(index, entry);
    }

    /// Reads a value from the Object Dictionary by index and sub-index.
    pub fn read<'s>(&'s self, index: u16, sub_index: u8) -> Option<Cow<'s, ObjectValue>> {
        self.entries.get(&index).and_then(|entry| match &entry.object {
            Object::Variable(value) => (sub_index == 0).then_some(Cow::Borrowed(value)),
            Object::Array(values) | Object::Record(values) => {
                if sub_index == 0 {
                    // Sub-index 0 holds the number of entries.
                    Some(Cow::Owned(ObjectValue::Unsigned8(values.len() as u8)))
                } else {
                    values.get(sub_index as usize - 1).map(Cow::Borrowed)
                }
            }
        })
    }

    // --- Start of Type-Safe Accessors ---
    pub fn read_u8(&self, index: u16, sub_index: u8) -> Option<u8> {
        self.read(index, sub_index)
            .and_then(|cow| if let ObjectValue::Unsigned8(val) = *cow { Some(val) } else { None })
    }

    pub fn read_u16(&self, index: u16, sub_index: u8) -> Option<u16> {
        self.read(index, sub_index)
            .and_then(|cow| if let ObjectValue::Unsigned16(val) = *cow { Some(val) } else { None })
    }

    pub fn read_u32(&self, index: u16, sub_index: u8) -> Option<u32> {
        self.read(index, sub_index)
            .and_then(|cow| if let ObjectValue::Unsigned32(val) = *cow { Some(val) } else { None })
    }
    // --- End of Type-Safe Accessors ---

    pub fn write(&mut self, index: u16, sub_index: u8, value: ObjectValue) -> Result<(), &'static str> {
        // Special case for Store Parameters command (1010h).
        if index == 0x1010 {
            if let ObjectValue::VisibleString(s) = &value {
                if s == "save" {
                    return self.store_parameters(sub_index);
                }
            }
            return Err("Invalid signature for Store Parameters");
        }

        // Special case for Restore Default Parameters command (1011h).
        if index == 0x1011 {
            if let ObjectValue::VisibleString(s) = &value {
                if s == "load" {
                    return self.restore_defaults(sub_index);
                }
            }
            return Err("Invalid signature for Restore Defaults");
        }
        self.write_internal(index, sub_index, value, true)
    }

    /// Internal write function with an option to bypass access checks.
    fn write_internal(&mut self, index: u16, sub_index: u8, value: ObjectValue, check_access: bool) -> Result<(), &'static str> {
        self.entries.get_mut(&index).map_or(Err("Object index not found"), |entry| {
            if check_access && matches!(entry.access, AccessType::ReadOnly | AccessType::Constant) {
                return Err("Object is read-only");
            }
            match &mut entry.object {
                Object::Variable(v) => {
                    if sub_index == 0 { *v = value; Ok(()) }
                    else { Err("Invalid sub-index for a variable") }
                }
                Object::Array(values) | Object::Record(values) => {
                    if sub_index == 0 { Err("Cannot write to sub-index 0") }
                    else if let Some(v) = values.get_mut(sub_index as usize - 1) { *v = value; Ok(()) }
                    else { Err("Sub-index out of bounds") }
                }
            }
        })
    }

    fn store_parameters(&mut self, list_to_save: u8) -> Result<(), &'static str> {
        if list_to_save == 0 { return Err("Cannot save to sub-index 0"); }
        if let Some(s) = &mut self.storage {
            let mut storable_params = BTreeMap::new();
            for (&index, entry) in &self.entries {
                if matches!(entry.access, AccessType::ReadWriteStore | AccessType::WriteOnlYStore) {
                    // A real implementation would filter based on `list_to_save`.
                    // For now, we save all storable parameters if sub-index 1 is used.
                    if list_to_save == 1 {
                        match &entry.object {
                            Object::Variable(val) => { storable_params.insert((index, 0), val.clone()); }
                            Object::Record(vals) | Object::Array(vals) => {
                                for (i, val) in vals.iter().enumerate() {
                                    storable_params.insert((index, (i + 1) as u8), val.clone());
                                }
                            }
                        }
                    }
                }
            }
            s.save(&storable_params)
        } else { Err("No storage backend configured") }
    }

    /// Tells the storage backend to clear persistent data.
    fn restore_defaults(&mut self, list_to_restore: u8) -> Result<(), &'static str> {
        if list_to_restore == 0 { return Err("Cannot restore from sub-index 0"); }
        if let Some(s) = &mut self.storage {
            // A real implementation would clear specific sets of parameters.
            // For now, sub-index 1 clears everything.
            if list_to_restore == 1 {
                s.clear()
            } else {
                Err("Unsupported restore list")
            }
        } else {
            Err("No storage backend configured")
        }
    }
}


// --- Unit Tests ---
#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    struct MockStorage {
        saved_data: BTreeMap<(u16, u8), ObjectValue>,
        save_called: bool, load_called: bool, clear_called: bool,
    }
    impl MockStorage { fn new() -> Self { Self { saved_data: BTreeMap::new(), save_called: false, load_called: false, clear_called: false } } }
    impl ObjectDictionaryStorage for MockStorage {
        fn load(&mut self) -> Result<BTreeMap<(u16, u8), ObjectValue>, &'static str> { self.load_called = true; Ok(self.saved_data.clone()) }
        fn save(&mut self, parameters: &BTreeMap<(u16, u8), ObjectValue>) -> Result<(), &'static str> { self.save_called = true; self.saved_data = parameters.clone(); Ok(()) }
        fn clear(&mut self) -> Result<(), &'static str> { self.clear_called = true; self.saved_data.clear(); Ok(()) }
    }

    #[test]
    fn test_new_populates_protocol_objects() {
        let od = ObjectDictionary::new(None);
        assert!(od.read(0x1010, 0).is_some());
        assert!(od.read(0x1011, 0).is_some());
    }

    #[test]
    fn test_loading_from_storage_on_new() {
        let mut storage = MockStorage::new();
        storage.saved_data.insert((0x6000, 0), ObjectValue::Unsigned32(999));
        {
            let mut od = ObjectDictionary::new(Some(&mut storage));
            od.insert(0x6000, ObjectEntry { object: Object::Variable(ObjectValue::Unsigned32(0)), name: "StorableVar", access: AccessType::ReadWriteStore });
            assert_eq!(*od.read(0x6000, 0).unwrap(), ObjectValue::Unsigned32(999));
        }
        assert!(storage.load_called);
    }
    
    #[test]
    fn test_save_command() {
        let mut storage = MockStorage::new();
        let mut od = ObjectDictionary::new(Some(&mut storage));
        od.insert(0x6000, ObjectEntry { object: Object::Variable(ObjectValue::Unsigned32(123)), name: "StorableVar", access: AccessType::ReadWriteStore });
        od.insert(0x7000, ObjectEntry { object: Object::Variable(ObjectValue::Unsigned32(456)), name: "NonStorableVar", access: AccessType::ReadWrite });
        od.write(0x1010, 1, ObjectValue::VisibleString("save".to_string())).unwrap();
        assert!(storage.save_called);
        assert_eq!(storage.saved_data.len(), 1);
        assert_eq!(storage.saved_data.get(&(0x6000, 0)), Some(&ObjectValue::Unsigned32(123)));
    }
    
    #[test]
    fn test_restore_defaults_command() {
        let mut storage = MockStorage::new();
        storage.saved_data.insert((0x6000, 0), ObjectValue::Unsigned32(999));
        let mut od = ObjectDictionary::new(Some(&mut storage));
        od.write(0x1011, 1, ObjectValue::VisibleString("load".to_string())).unwrap();
        assert!(storage.clear_called);
        assert!(storage.saved_data.is_empty());
    }

    #[test]
    fn test_write_to_readonly_fails() {
        let mut od = ObjectDictionary::new(None);
        od.insert(0x1008, ObjectEntry { object: Object::Variable(ObjectValue::Unsigned8(10)), name: "ReadOnlyVar", access: AccessType::ReadOnly });
        let result = od.write(0x1008, 0, ObjectValue::Unsigned8(42));
        assert_eq!(result, Err("Object is read-only"));
        assert_eq!(*od.read(0x1008, 0).unwrap(), ObjectValue::Unsigned8(10));
    }

    #[test]
    fn test_read_sub_index_zero_for_array() {
        let mut od = ObjectDictionary::new(None);
        od.insert(0x2000, ObjectEntry { object: Object::Array(vec![ObjectValue::Unsigned16(100), ObjectValue::Unsigned16(200)]), name: "TestArray", access: AccessType::ReadWrite });
        let value = od.read(0x2000, 0).unwrap();
        assert_eq!(*value, ObjectValue::Unsigned8(2));
        assert!(matches!(value, Cow::Owned(_)));
    }

    #[test]
    fn test_type_safe_accessors() {
        let mut od = ObjectDictionary::new(None);
        od.insert(0x6000, ObjectEntry { object: Object::Variable(ObjectValue::Unsigned32(12345)), name: "TestU32", access: AccessType::ReadOnly });
        od.insert(0x6001, ObjectEntry { object: Object::Variable(ObjectValue::Unsigned8(99)), name: "TestU8", access: AccessType::ReadOnly });
        
        assert_eq!(od.read_u32(0x6000, 0), Some(12345));
        assert_eq!(od.read_u8(0x6001, 0), Some(99));
        // Test wrong type
        assert_eq!(od.read_u16(0x6000, 0), None);
        // Test wrong index
        assert_eq!(od.read_u32(0x9999, 0), None);
    }
}
