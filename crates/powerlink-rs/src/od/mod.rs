use crate::common::{NetTime, TimeDifference, TimeOfDay};
use crate::hal::ObjectDictionaryStorage;
use crate::types::{
    BOOLEAN, INTEGER16, INTEGER32, INTEGER64, INTEGER8, REAL32, REAL64, UNSIGNED16, UNSIGNED32,
    UNSIGNED64, UNSIGNED8, IpAddress, MacAddress,
};
use crate::PowerlinkError;
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
    /// Creates a new, empty OD.
    /// Call `init()` after populating with application and device defaults
    /// to load persistent parameters and finish setup.
    pub fn new(storage: Option<&'a mut dyn ObjectDictionaryStorage>) -> Self {
        Self {
            entries: BTreeMap::new(),
            storage,
        }
    }

    /// Initialises the Object Dictionary.
    /// This method must be called after the application has inserted all its
    /// default and device-specific objects. It performs the following actions:
    /// 1. Populates mandatory communication profile objects.
    /// 2. Checks if a "Restore Defaults" command was flagged in storage.
    /// 3. If so, clears storage and proceeds with firmware defaults.
    /// 4. If not, loads all stored parameters from the backend.
    pub fn init(&mut self) -> Result<(), PowerlinkError> {
        let mut restore_defaults = false;
        if let Some(s) = &mut self.storage {
            if s.restore_defaults_requested() {
                restore_defaults = true;
                s.clear_restore_defaults_flag()?;
                s.clear()?;
            }
        }

        self.populate_protocol_objects();

        if !restore_defaults {
            self.load()?;
        }
        Ok(())
    }

    /// Populates the OD with mandatory objects that define protocol mechanisms.
    /// Device-specific identification objects are left to the user to insert.
    fn populate_protocol_objects(&mut self) {
        // Add Store Parameters (1010h) as a RECORD.
        self.insert(
            0x1010,
            ObjectEntry {
                object: Object::Record(vec![
                    ObjectValue::Unsigned32(1), // Sub-index 1: Save All Parameters
                    ObjectValue::Unsigned32(1), // Sub-index 2: Save Communication Parameters
                    ObjectValue::Unsigned32(1), // Sub-index 3: Save Application Parameters
                ]),
                name: "NMT_StoreParam_REC",
                access: AccessType::ReadWrite,
            },
        );

        // Add Restore Default Parameters (1011h) as a RECORD.
        self.insert(
            0x1011,
            ObjectEntry {
                object: Object::Record(vec![
                    ObjectValue::Unsigned32(1), // Sub-index 1: Restore All Parameters
                    ObjectValue::Unsigned32(1), // Sub-index 2: Restore Communication Parameters
                    ObjectValue::Unsigned32(1), // Sub-index 3: Restore Application Parameters
                ]),
                name: "NMT_RestoreDefParam_REC",
                access: AccessType::ReadWrite,
            },
        );

        // Add current NMT state object (1F8Ch).
        self.insert(
            0x1F8C,
            ObjectEntry {
                object: Object::Variable(ObjectValue::Unsigned8(0)),
                name: "NMT_CurrNMTState_U8",
                access: AccessType::ReadOnly,
            },
        );
    }

    /// Validates that the OD contains all mandatory objects required for a node to function.
    /// Should be called from the node's constructor.
    pub fn validate_mandatory_objects(&self) -> Result<(), PowerlinkError> {
        const MANDATORY_OBJECTS: &[u16] = &[
            0x1000, // NMT_DeviceType_U32
            0x1018, // NMT_IdentityObject_REC
            0x1F82, // NMT_FeatureFlags_U32
            0x1F93, // NMT_EPLNodeID_REC
            0x1F99, // NMT_CNBasicEthernetTimeout_U32
        ];
        for &index in MANDATORY_OBJECTS {
            if !self.entries.contains_key(&index) {
                return Err(PowerlinkError::ObjectNotFound);
            }
        }
        Ok(())
    }

    /// Loads values from the persistent storage backend and overwrites any
    /// matching existing entries in the OD. This is called by `init()`.
    fn load(&mut self) -> Result<(), PowerlinkError> {
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

    /// Inserts a new object entry into the dictionary at a given index.
    pub fn insert(&mut self, index: u16, entry: ObjectEntry) {
        self.entries.insert(index, entry);
    }

    /// Reads a value from the Object Dictionary by index and sub-index.
    pub fn read<'s>(&'s self, index: u16, sub_index: u8) -> Option<Cow<'s, ObjectValue>> {
        self.entries
            .get(&index)
            .and_then(|entry| match &entry.object {
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

    // --- Start of Type-Safe Accessors ---
    pub fn read_u8(&self, index: u16, sub_index: u8) -> Option<u8> {
        self.read(index, sub_index).and_then(|cow| {
            if let ObjectValue::Unsigned8(val) = *cow {
                Some(val)
            } else {
                None
            }
        })
    }

    pub fn read_u16(&self, index: u16, sub_index: u8) -> Option<u16> {
        self.read(index, sub_index).and_then(|cow| {
            if let ObjectValue::Unsigned16(val) = *cow {
                Some(val)
            } else {
                None
            }
        })
    }

    pub fn read_u32(&self, index: u16, sub_index: u8) -> Option<u32> {
        self.read(index, sub_index).and_then(|cow| {
            if let ObjectValue::Unsigned32(val) = *cow {
                Some(val)
            } else {
                None
            }
        })
    }
    // --- End of Type-Safe Accessors ---

    /// Public write function that respects access rights and handles special command objects.
    pub fn write(
        &mut self,
        index: u16,
        sub_index: u8,
        value: ObjectValue,
    ) -> Result<(), PowerlinkError> {
        // Special case for Store Parameters command (1010h).
        if index == 0x1010 {
            if let ObjectValue::VisibleString(s) = &value {
                if s == "save" {
                    return self.store_parameters(sub_index);
                }
            }
            return Err(PowerlinkError::StorageError(
                "Invalid signature for Store Parameters",
            ));
        }

        // Special case for Restore Default Parameters command (1011h).
        if index == 0x1011 {
            if let ObjectValue::VisibleString(s) = &value {
                if s == "load" {
                    return self.restore_defaults(sub_index);
                }
            }
            return Err(PowerlinkError::StorageError(
                "Invalid signature for Restore Defaults",
            ));
        }
        self.write_internal(index, sub_index, value, true)
    }

    /// Internal write function with an option to bypass access checks.
    pub(super) fn write_internal(
        &mut self,
        index: u16,
        sub_index: u8,
        value: ObjectValue,
        check_access: bool,
    ) -> Result<(), PowerlinkError> {
        self.entries
            .get_mut(&index)
            .map_or(Err(PowerlinkError::ObjectNotFound), |entry| {
                if check_access
                    && matches!(entry.access, AccessType::ReadOnly | AccessType::Constant)
                {
                    return Err(PowerlinkError::StorageError("Object is read-only"));
                }
                match &mut entry.object {
                    Object::Variable(v) => {
                        if sub_index == 0 {
                            *v = value;
                            Ok(())
                        } else {
                            Err(PowerlinkError::SubObjectNotFound)
                        }
                    }
                    Object::Array(values) | Object::Record(values) => {
                        if sub_index == 0 {
                            Err(PowerlinkError::StorageError("Cannot write to sub-index 0"))
                        } else if let Some(v) = values.get_mut(sub_index as usize - 1) {
                            *v = value;
                            Ok(())
                        } else {
                            Err(PowerlinkError::SubObjectNotFound)
                        }
                    }
                }
            })
    }

    /// Collects all storable parameters and tells the storage backend to save them.
    fn store_parameters(&mut self, list_to_save: u8) -> Result<(), PowerlinkError> {
        if list_to_save == 0 {
            return Err(PowerlinkError::StorageError("Cannot save to sub-index 0"));
        }
        if let Some(s) = &mut self.storage {
            let mut storable_params = BTreeMap::new();
            for (&index, entry) in &self.entries {
                // Determine if this object's group (Comm, App, etc.) should be saved.
                let should_save = match list_to_save {
                    1 => true,                                     // Save All
                    2 => (0x1000..=0x1FFF).contains(&index), // Save Communication
                    3 => (0x6000..=0x9FFF).contains(&index), // Save Application
                    _ => false, // Other sub-indices are manufacturer-specific
                };

                if should_save
                    && matches!(
                        entry.access,
                        AccessType::ReadWriteStore | AccessType::WriteOnlyStore
                    )
                {
                    match &entry.object {
                        Object::Variable(val) => {
                            storable_params.insert((index, 0), val.clone());
                        }
                        Object::Record(vals) | Object::Array(vals) => {
                            for (i, val) in vals.iter().enumerate() {
                                storable_params.insert((index, (i + 1) as u8), val.clone());
                            }
                        }
                    }
                }
            }
            s.save(&storable_params)
        } else {
            Err(PowerlinkError::StorageError(
                "No storage backend configured",
            ))
        }
    }

    /// Tells the storage backend to set a flag to restore defaults on the next boot.
    /// The actual data clearing happens at startup.
    fn restore_defaults(&mut self, list_to_restore: u8) -> Result<(), PowerlinkError> {
        if list_to_restore == 0 {
            return Err(PowerlinkError::StorageError(
                "Cannot restore from sub-index 0",
            ));
        }
        if let Some(s) = &mut self.storage {
            // For now, any valid "load" command flags a full restore.
            // A full implementation could pass `list_to_restore` to the storage backend.
            s.request_restore_defaults()
        } else {
            Err(PowerlinkError::StorageError(
                "No storage backend configured",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hal::ObjectDictionaryStorage;
    use alloc::string::ToString;
    use alloc::vec;

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
        fn load(&mut self) -> Result<BTreeMap<(u16, u8), ObjectValue>, PowerlinkError> {
            self.load_called = true;
            Ok(self.saved_data.clone())
        }
        fn save(
            &mut self,
            params: &BTreeMap<(u16, u8), ObjectValue>,
        ) -> Result<(), PowerlinkError> {
            self.save_called = true;
            self.saved_data = params.clone();
            Ok(())
        }
        fn clear(&mut self) -> Result<(), PowerlinkError> {
            self.clear_called = true;
            self.saved_data.clear();
            Ok(())
        }
        fn restore_defaults_requested(&self) -> bool {
            self.restore_requested
        }
        fn request_restore_defaults(&mut self) -> Result<(), PowerlinkError> {
            self.restore_requested = true;
            Ok(())
        }
        fn clear_restore_defaults_flag(&mut self) -> Result<(), PowerlinkError> {
            self.restore_requested = false;
            Ok(())
        }
    }

    #[test]
    fn test_read_variable() {
        let mut od = ObjectDictionary::new(None);
        od.insert(
            0x1006,
            ObjectEntry {
                object: Object::Variable(ObjectValue::Unsigned32(12345)),
                name: "TestVar",
                access: AccessType::ReadWrite,
            },
        );

        let value = od.read(0x1006, 0).unwrap();
        assert_eq!(*value, ObjectValue::Unsigned32(12345));
    }

    #[test]
    fn test_read_write_array_element() {
        let mut od = ObjectDictionary::new(None);
        od.insert(
            0x2000,
            ObjectEntry {
                object: Object::Array(vec![ObjectValue::Unsigned16(100)]),
                name: "TestArray",
                access: AccessType::ReadWrite,
            },
        );

        od.write(0x2000, 1, ObjectValue::Unsigned16(999)).unwrap();
        let value = od.read(0x2000, 1).unwrap();
        assert_eq!(*value, ObjectValue::Unsigned16(999));
    }

    #[test]
    fn test_read_sub_index_zero_returns_owned_length() {
        let mut od = ObjectDictionary::new(None);
        od.insert(
            0x2000,
            ObjectEntry {
                object: Object::Array(vec![
                    ObjectValue::Unsigned16(100),
                    ObjectValue::Unsigned16(200),
                ]),
                name: "TestArray",
                access: AccessType::ReadWrite,
            },
        );

        let value = od.read(0x2000, 0).unwrap();
        assert_eq!(*value, ObjectValue::Unsigned8(2));
        assert!(matches!(value, Cow::Owned(_)));
    }

    #[test]
    fn test_write_to_readonly_fails() {
        let mut od = ObjectDictionary::new(None);
        od.insert(
            0x1008,
            ObjectEntry {
                object: Object::Variable(ObjectValue::Unsigned8(10)),
                name: "ReadOnlyVar",
                access: AccessType::ReadOnly,
            },
        );

        let result = od.write(0x1008, 0, ObjectValue::Unsigned8(42));
        assert!(result.is_err());
        assert_eq!(*od.read(0x1008, 0).unwrap(), ObjectValue::Unsigned8(10));
    }

    #[test]
    fn test_save_command() {
        let mut storage = MockStorage::new();
        {
            let mut od = ObjectDictionary::new(Some(&mut storage));
            od.insert(
                0x6000,
                ObjectEntry {
                    object: Object::Variable(ObjectValue::Unsigned32(123)),
                    name: "StorableVar",
                    access: AccessType::ReadWriteStore,
                },
            );

            od.write(0x1010, 1, ObjectValue::VisibleString("save".to_string()))
                .unwrap();
        }

        assert!(storage.save_called);
        assert_eq!(
            storage.saved_data.get(&(0x6000, 0)),
            Some(&ObjectValue::Unsigned32(123))
        );
    }

    #[test]
    fn test_restore_defaults_command_flags_for_reboot() {
        let mut storage = MockStorage::new();
        {
            let mut od = ObjectDictionary::new(Some(&mut storage));
            assert!(od.storage.unwrap().restore_defaults_requested());
            od.write(0x1011, 1, ObjectValue::VisibleString("load".to_string()))
                .unwrap();
        } // od is dropped here, releasing the borrow

        assert!(storage.restore_requested);
        assert!(!storage.clear_called);
    }

    #[test]
    fn test_loading_from_storage_on_init() {
        let mut storage = MockStorage::new();
        storage
            .saved_data
            .insert((0x6000, 0), ObjectValue::Unsigned32(999));

        let mut od = ObjectDictionary::new(Some(&mut storage));
        od.insert(
            0x6000,
            ObjectEntry {
                object: Object::Variable(ObjectValue::Unsigned32(0)),
                name: "StorableVar",
                access: AccessType::ReadWriteStore,
            },
        );

        od.init().unwrap();       

        assert_eq!(od.read_u32(0x6000, 0).unwrap(), 999);
    }

    #[test]
    fn test_init_restores_defaults_if_flagged() {
        let mut storage = MockStorage::new();
        storage
            .saved_data
            .insert((0x6000, 0), ObjectValue::Unsigned32(999));
        storage.restore_requested = true;

        let mut od = ObjectDictionary::new(Some(&mut storage));
        od.insert(
            0x6000,
            ObjectEntry {
                object: Object::Variable(ObjectValue::Unsigned32(0)), // Firmware default
                name: "StorableVar",
                access: AccessType::ReadWriteStore,
            },
        );

        od.init().unwrap();

        assert!(!od.storage.unwrap().restore_defaults_requested()); // Flag should be cleared

        assert_eq!(od.read_u32(0x6000, 0).unwrap(), 0); // Back to default
    }
}

