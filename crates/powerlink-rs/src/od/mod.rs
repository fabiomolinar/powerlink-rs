use crate::hal::ObjectDictionaryStorage;
use crate::common::{NetTime, TimeDifference, TimeOfDay};
use crate::types::{
    BOOLEAN, INTEGER16, INTEGER32, INTEGER64, INTEGER8, REAL32, REAL64, UNSIGNED16,
    UNSIGNED32, UNSIGNED64, UNSIGNED8, IpAddress, MacAddress,
};
use crate::PowerlinkError;
use alloc::{
    borrow::Cow,
    collections::BTreeMap,
    string::String,
    vec,
    vec::Vec,
};
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

/// Groups of parameters that can be saved or restored, corresponding to
/// the sub-indices of objects 0x1010 and 0x1011.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParameterGroup {
    All = 1,
    Communication = 2,
    Application = 3,
    // Manufacturer-specific groups would be defined from 4 upwards.
}

impl TryFrom<u8> for ParameterGroup {
    type Error = &'static str;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::All),
            2 => Ok(Self::Communication),
            3 => Ok(Self::Application),
            _ => Err("Invalid or unsupported parameter group"),
        }
    }
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
    /// Creates a new, empty OD with an optional storage backend.
    /// The OD must be initialized with `init()` after device-specific objects are inserted.
    pub fn new(storage: Option<&'a mut dyn ObjectDictionaryStorage>) -> Self {
        Self {
            entries: BTreeMap::new(),
            storage,
        }
    }

    /// Initializes the OD by populating protocol objects and then either
    /// restoring defaults or loading from persistent storage.
    /// This must be called after the application inserts its device-specific objects.
    pub fn init(&mut self) -> Result<(), &'static str> {
        self.populate_protocol_objects();

        if let Some(s) = &mut self.storage {
            if s.is_restore_requested()? {
                // Restore defaults was requested on a previous boot cycle.
                s.clear()?;
                s.clear_restore_request()?;
            } else {
                // Normal boot: load saved parameters from storage.
                self.load()?;
            }
        }
        Ok(())
    }

    /// Validates that all mandatory device-specific objects have been inserted
    /// by the application. This should be called from the `Node` constructor.
    pub fn validate_mandatory_objects(&self) -> Result<(), PowerlinkError> {
        const MANDATORY_OBJECTS: &[(u16, &'static str)] = &[
            (0x1000, "NMT_DeviceType_U32"),
            (0x1018, "NMT_IdentityObject_REC"),
            (0x1F82, "NMT_FeatureFlags_U32"),
            (0x1F93, "NMT_EPLNodeID_REC"),
            (0x1F99, "NMT_CNBasicEthernetTimeout_U32"),
        ];
        for (index, name) in MANDATORY_OBJECTS {
            if !self.entries.contains_key(index) {
                return Err(PowerlinkError::ValidationError(name));
            }
        }
        Ok(())
    }

    /// Loads values from the persistent storage backend and overwrites any
    /// matching existing entries in the OD.
    fn load(&mut self) -> Result<(), &'static str> {
        if let Some(s) = &mut self.storage {
            let stored_params = s.load()?;
            for ((index, sub_index), value) in stored_params {
                // Attempt to write the loaded value. Ignore errors for objects
                // that might exist in storage but not in the current firmware.
                let _ = self.write_internal(index, sub_index, value, false);
            }
        }
        Ok(())
    }

    /// Populates the OD with mandatory objects defined by the POWERLINK
    /// communication profile, which are common to all devices. This ensures
    /// that core protocol mechanisms are always available.
    fn populate_protocol_objects(&mut self) {
        // Add Store Parameters (0x1010)
        self.insert(0x1010, ObjectEntry {
            object: Object::Record(vec![
                ObjectValue::Unsigned32(1), // Sub-index 1: AllParam
                ObjectValue::Unsigned32(1), // Sub-index 2: CommunicationParam
                ObjectValue::Unsigned32(1), // Sub-index 3: ApplicationParam
            ]),
            name: "NMT_StoreParam_REC",
            access: AccessType::ReadWrite,
        });

        // Add Restore Default Parameters (0x1011)
        self.insert(0x1011, ObjectEntry {
            object: Object::Record(vec![
                ObjectValue::Unsigned32(1), // Sub-index 1: AllParam
                ObjectValue::Unsigned32(1), // Sub-index 2: CommunicationParam
                ObjectValue::Unsigned32(1), // Sub-index 3: ApplicationParam
            ]),
            name: "NMT_RestoreDefParam_REC",
            access: AccessType::ReadWrite,
        });

        // Add Current NMT State (0x1F8C) for diagnostic purposes.
        self.insert(0x1F8C, ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned8(0)),
            name: "NMT_CurrNMTState_U8",
            access: AccessType::ReadOnly,
        });
    }

    /// Inserts a new object entry into the dictionary at a given index.
    pub fn insert(&mut self, index: u16, entry: ObjectEntry) {
        self.entries.insert(index, entry);
    }

    /// Reads a value from the Object Dictionary by index and sub-index.
    pub fn read<'s>(&'s self, index: u16, sub_index: u8) -> Option<Cow<'s, ObjectValue>> {
        self.entries.get(&index).and_then(|entry| match &entry.object {
            Object::Variable(value) => {
                if sub_index == 0 { Some(Cow::Borrowed(value)) } else { None }
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

    /// Public write function that respects access rights and handles special command objects.
    pub fn write(&mut self, index: u16, sub_index: u8, value: ObjectValue) -> Result<(), &'static str> {
        // Special case for Store Parameters command (0x1010).
        if index == 0x1010 {
            if let ObjectValue::VisibleString(s) = &value {
                if s == "save" {
                    let group = ParameterGroup::try_from(sub_index)?;
                    return self.store_parameters(group);
                }
            }
            return Err("Invalid signature for Store Parameters");
        }

        // Special case for Restore Default Parameters command (0x1011).
        if index == 0x1011 {
            if let ObjectValue::VisibleString(s) = &value {
                if s == "load" {
                    let group = ParameterGroup::try_from(sub_index)?;
                    return self.restore_defaults(group);
                }
            }
            return Err("Invalid signature for Restore Defaults");
        }
        self.write_internal(index, sub_index, value, true)
    }

    /// Internal write function with an option to bypass access checks.
    pub(super) fn write_internal(&mut self, index: u16, sub_index: u8, value: ObjectValue, check_access: bool) -> Result<(), &'static str> {
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

    /// Collects all storable parameters and tells the storage backend to save them.
    fn store_parameters(&mut self, group: ParameterGroup) -> Result<(), &'static str> {
        if let Some(s) = &mut self.storage {
            let mut storable_params = BTreeMap::new();
            for (&index, entry) in &self.entries {
                if matches!(entry.access, AccessType::ReadWriteStore | AccessType::WriteOnlYStore) {
                    let in_group = match group {
                        ParameterGroup::All => true,
                        ParameterGroup::Communication => (0x1000..=0x1FFF).contains(&index),
                        ParameterGroup::Application => (0x6000..=0x9FFF).contains(&index),
                    };

                    if in_group {
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

    /// Sets a persistent flag to restore defaults on the next reboot.
    fn restore_defaults(&mut self, group: ParameterGroup) -> Result<(), &'static str> {
        // For now, any restore group request triggers a full restore.
        // A more advanced implementation could set different flags for different groups.
        if group == ParameterGroup::All {
             if let Some(s) = &mut self.storage {
                s.request_restore_defaults()
            } else {
                Err("No storage backend configured")
            }
        } else {
            Err("Selective restore not yet implemented")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hal::ObjectDictionaryStorage;
    use alloc::string::ToString;
    
    struct MockStorage { 
        saved_data: BTreeMap<(u16, u8), ObjectValue>, 
        restore_requested: bool,
        save_called: bool,
        load_called: bool,
        clear_called: bool,
    }
    impl MockStorage { 
        fn new() -> Self { 
            Self { 
                saved_data: BTreeMap::new(), 
                restore_requested: false,
                save_called: false,
                load_called: false,
                clear_called: false,
            } 
        } 
    }
    impl ObjectDictionaryStorage for MockStorage {
        fn load(&mut self) -> Result<BTreeMap<(u16, u8), ObjectValue>, &'static str> { self.load_called = true; Ok(self.saved_data.clone()) }
        fn save(&mut self, params: &BTreeMap<(u16, u8), ObjectValue>) -> Result<(), &'static str> { self.save_called = true; self.saved_data = params.clone(); Ok(()) }
        fn clear(&mut self) -> Result<(), &'static str> { self.clear_called = true; self.saved_data.clear(); Ok(()) }
        fn is_restore_requested(&mut self) -> Result<bool, &'static str> { Ok(self.restore_requested) }
        fn request_restore_defaults(&mut self) -> Result<(), &'static str> { self.restore_requested = true; Ok(()) }
        fn clear_restore_request(&mut self) -> Result<(), &'static str> { self.restore_requested = false; Ok(()) }
    }

    #[test]
    fn test_read_variable() {
        let mut od = ObjectDictionary::new(None);
        od.insert(0x1006, ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned32(12345)),
            name: "TestVar",
            access: AccessType::ReadWrite,
        });

        let value = od.read(0x1006, 0).unwrap();
        assert_eq!(*value, ObjectValue::Unsigned32(12345));
    }

    #[test]
    fn test_read_write_array_element() {
        let mut od = ObjectDictionary::new(None);
        od.insert(0x2000, ObjectEntry { 
            object: Object::Array(vec![ObjectValue::Unsigned16(100)]), 
            name: "TestArray", 
            access: AccessType::ReadWrite
        });

        od.write(0x2000, 1, ObjectValue::Unsigned16(999)).unwrap();
        let value = od.read(0x2000, 1).unwrap();
        assert_eq!(*value, ObjectValue::Unsigned16(999));
    }

    #[test]
    fn test_read_sub_index_zero_returns_owned_length() {
        let mut od = ObjectDictionary::new(None);
        od.insert(0x2000, ObjectEntry { 
            object: Object::Array(vec![ObjectValue::Unsigned16(100), ObjectValue::Unsigned16(200)]), 
            name: "TestArray", 
            access: AccessType::ReadWrite
        });

        let value = od.read(0x2000, 0).unwrap();
        assert_eq!(*value, ObjectValue::Unsigned8(2));
        assert!(matches!(value, Cow::Owned(_)));
    }

    #[test]
    fn test_write_to_readonly_fails() {
        let mut od = ObjectDictionary::new(None);
        od.insert(0x1008, ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned8(10)),
            name: "ReadOnlyVar",
            access: AccessType::ReadOnly,
        });

        let result = od.write(0x1008, 0, ObjectValue::Unsigned8(42));
        assert_eq!(result, Err("Object is read-only"));
        assert_eq!(*od.read(0x1008, 0).unwrap(), ObjectValue::Unsigned8(10));
    }

    #[test]
    fn test_save_command_all_params() {
        let mut storage = MockStorage::new();
        let mut od = ObjectDictionary::new(Some(&mut storage));
        od.insert(0x6000, ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned32(123)),
            name: "StorableVar",
            access: AccessType::ReadWriteStore,
        });

        od.write(0x1010, 1, ObjectValue::VisibleString("save".to_string())).unwrap();

        assert!(storage.save_called);
        assert_eq!(storage.saved_data.get(&(0x6000, 0)), Some(&ObjectValue::Unsigned32(123)));
    }
    
    #[test]
    fn test_save_command_comm_params_only() {
        let mut storage = MockStorage::new();
        let mut od = ObjectDictionary::new(Some(&mut storage));
        // Communication parameter
        od.insert(0x1F99, ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned32(5000)),
            name: "CommParam",
            access: AccessType::ReadWriteStore,
        });
        // Application parameter (should not be saved)
        od.insert(0x6000, ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned32(123)),
            name: "AppVar",
            access: AccessType::ReadWriteStore,
        });

        od.write(0x1010, 2, ObjectValue::VisibleString("save".to_string())).unwrap();

        assert!(storage.save_called);
        assert_eq!(storage.saved_data.get(&(0x1F99, 0)), Some(&ObjectValue::Unsigned32(5000)));
        assert!(storage.saved_data.get(&(0x6000, 0)).is_none());
    }

    #[test]
    fn test_restore_defaults_command_requests_restore() {
        let mut storage = MockStorage::new();
        assert!(!storage.restore_requested);

        {
            let mut od = ObjectDictionary::new(Some(&mut storage));
            od.write(0x1011, 1, ObjectValue::VisibleString("load".to_string())).unwrap();
        } // `od` is dropped here, releasing the mutable borrow.

        assert!(storage.restore_requested);
        assert!(!storage.clear_called); // Should not clear immediately
    }

    #[test]
    fn test_init_loads_from_storage_on_normal_boot() {
        let mut storage = MockStorage::new();
        storage.saved_data.insert((0x6000, 0), ObjectValue::Unsigned32(999));
        
        let od_val = { // Scope for od
            let mut od = ObjectDictionary::new(Some(&mut storage));
            od.insert(0x6000, ObjectEntry {
                object: Object::Variable(ObjectValue::Unsigned32(0)),
                name: "StorableVar",
                access: AccessType::ReadWriteStore,
            });
            
            // Value is default before init.
            assert_eq!(od.read_u32(0x6000, 0).unwrap(), 0);
            
            // Init from storage.
            od.init().unwrap();

            // Value is now updated.
            od.read_u32(0x6000, 0).unwrap()
        };
        assert_eq!(od_val, 999);
        
        assert!(storage.load_called);
        assert!(!storage.clear_called);
    }
    
    #[test]
    fn test_init_restores_defaults_when_flag_is_set() {
        let mut storage = MockStorage::new();
        storage.saved_data.insert((0x6000, 0), ObjectValue::Unsigned32(999));
        storage.restore_requested = true;
        
        let od_val = {
            let mut od = ObjectDictionary::new(Some(&mut storage));
            od.insert(0x6000, ObjectEntry {
                object: Object::Variable(ObjectValue::Unsigned32(0)), // Firmware default
                name: "StorableVar",
                access: AccessType::ReadWriteStore,
            });
            
            od.init().unwrap(); // Should trigger restore
            od.read_u32(0x6000, 0).unwrap()
        };
        
        assert!(!storage.load_called);
        assert!(storage.clear_called);
        assert!(!storage.restore_requested); // Flag should be cleared
        assert_eq!(od_val, 0); // Back to default
    }

    #[test]
    fn test_validation_succeeds() {
        let mut od = ObjectDictionary::new(None);
        od.insert(0x1000, ObjectEntry::default());
        od.insert(0x1018, ObjectEntry::default());
        od.insert(0x1F82, ObjectEntry::default());
        od.insert(0x1F93, ObjectEntry::default());
        od.insert(0x1F99, ObjectEntry::default());

        assert!(od.validate_mandatory_objects().is_ok());
    }

    #[test]
    fn test_validation_fails() {
        let od = ObjectDictionary::new(None);
        let result = od.validate_mandatory_objects();
        assert!(matches!(result, Err(PowerlinkError::ValidationError("NMT_DeviceType_U32"))));
    }

    // Default impl for tests
    impl Default for ObjectEntry {
        fn default() -> Self {
            Self {
                object: Object::Variable(ObjectValue::Unsigned8(0)),
                name: "Default",
                access: AccessType::ReadWrite,
            }
        }
    }
}

