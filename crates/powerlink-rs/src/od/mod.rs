// crates/powerlink-rs/src/od/mod.rs

mod entry;
mod predefined;
mod value;

pub use entry::{AccessType, Category, Object, ObjectEntry, PdoMapping, ValueRange};
pub use value::ObjectValue;

use crate::hal::ObjectDictionaryStorage;
use crate::{NodeId, PowerlinkError, pdo::PdoMappingEntry}; // Added PdoMappingEntry
use alloc::{borrow::Cow, collections::BTreeMap, vec::Vec};
use core::fmt;
use log::{error, trace}; // Added error and trace

/// The main Object Dictionary structure.
pub struct ObjectDictionary<'a> {
    pub(super) entries: BTreeMap<u16, ObjectEntry>,
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

        predefined::populate_protocol_objects(self);

        if !restore_defaults {
            self.load()?;
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

    /// Validates that the OD contains all mandatory objects required for a node to function.
    pub fn validate_mandatory_objects(&self, is_mn: bool) -> Result<(), PowerlinkError> {
        predefined::validate_mandatory_objects(self, is_mn)
    }

    /// Gets a list of configured isochronous CNs from object 0x1F81.
    pub fn get_configured_cns(&self) -> Vec<NodeId> {
        let mut cn_list = Vec::new();
        if let Some(entry) = self.entries.get(&0x1F81) {
            if let Object::Array(values) = &entry.object {
                // Sub-index 0 holds the count, so we iterate from the actual data.
                for (i, value) in values.iter().enumerate().skip(1) {
                    if let ObjectValue::Unsigned32(assignment) = value {
                        // Bit 0: Node exists
                        // Bit 8: Node is isochronous
                        if (assignment & (1 << 0)) != 0 && (assignment & (1 << 8)) == 0 {
                            cn_list.push(NodeId(i as u8));
                        }
                    }
                }
            }
        }
        cn_list
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

    /// Reads an object's enum (`Object::Variable`, `Object::Array`, etc.) by index.
    /// This is a helper for accessing the structural part of an entry.
    pub fn read_object(&self, index: u16) -> Option<&Object> {
        self.entries.get(&index).map(|entry| &entry.object)
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

    pub fn read_u64(&self, index: u16, sub_index: u8) -> Option<u64> {
        self.read(index, sub_index).and_then(|cow| {
            if let ObjectValue::Unsigned64(val) = *cow {
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
        
        // --- Special validation for PDO mapping ---
        if sub_index == 0 && ((0x1600..=0x16FF).contains(&index) || (0x1A00..=0x1AFF).contains(&index)) {
            if let ObjectValue::Unsigned8(new_num_entries) = value {
                // This is a write to NumberOfEntries of a mapping object. Validate it before committing.
                self.validate_pdo_mapping(index, new_num_entries)?;
            } else {
                // NumberOfEntries must always be a U8 value.
                return Err(PowerlinkError::TypeMismatch);
            }
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
                if check_access {
                    if let Some(access) = entry.access {
                        if matches!(access, AccessType::ReadOnly | AccessType::Constant) {
                            return Err(PowerlinkError::StorageError("Object is read-only"));
                        }
                    }
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
    
    /// Validates that a new PDO mapping configuration does not exceed payload size limits.
    /// This should be called *before* writing to NumberOfEntries (sub-index 0) of a mapping object.
    fn validate_pdo_mapping(&self, index: u16, new_num_entries: u8) -> Result<(), PowerlinkError> {
        if new_num_entries == 0 {
            return Ok(()); // Deactivating a mapping is always valid.
        }
    
        let is_tpdo = (0x1A00..=0x1AFF).contains(&index);
        let is_rpdo = (0x1600..=0x16FF).contains(&index);
        if !is_tpdo && !is_rpdo {
            return Ok(()); // Not a mapping object, no validation needed here.
        }
    
        // --- 1. Determine the HARD payload size limit for this PDO channel ---
        // Per spec 6.4.8.2, this check is against the max buffer size, not the actual frame size.
        let payload_limit_bytes = if is_tpdo {
            // TPDOs are checked against the max isochronous TRANSMIT buffer size.
            self.read_u16(0x1F98, 1).unwrap_or(1490) as usize // IsochrTxMaxPayload_U16
        } else { // is_rpdo
            // RPDOs are checked against the max isochronous RECEIVE buffer size.
            self.read_u16(0x1F98, 2).unwrap_or(1490) as usize // IsochrRxMaxPayload_U16
        };
    
        // --- 2. Calculate the required size from the existing mapping entries ---
        // The spec implies that mapping entries are written *before* NumberOfEntries is set.
        let mut max_bits_required: u32 = 0;
        if let Some(Object::Array(entries)) = self.read_object(index) {
            // Iterate up to the *new* number of entries that is being proposed.
            for i in 0..(new_num_entries as usize) {
                if let Some(ObjectValue::Unsigned64(raw_mapping)) = entries.get(i) {
                    let entry = PdoMappingEntry::from_u64(*raw_mapping);
                    let end_pos_bits = entry.offset_bits as u32 + entry.length_bits as u32;
                    if end_pos_bits > max_bits_required {
                        max_bits_required = end_pos_bits;
                    }
                } else {
                    // This indicates a configuration error: trying to enable more entries than have been written.
                    // This is an invalid state, but we can treat it as a validation failure.
                    return Err(PowerlinkError::ValidationError("Incomplete PDO mapping configuration"));
                }
            }
        } else {
             // This should not happen if called correctly, as the object must exist.
            return Err(PowerlinkError::ObjectNotFound);
        }
    
        // Convert bits to bytes, rounding up.
        let required_bytes = (max_bits_required + 7) / 8;
    
        // --- 3. Compare and return result ---
        if required_bytes as usize > payload_limit_bytes {
            error!(
                "PDO mapping validation failed for index {:#06X}. Required size: {} bytes, Hard Limit: {} bytes. [E_PDO_MAP_OVERRUN]",
                index, required_bytes, payload_limit_bytes
            );
            Err(PowerlinkError::PdoMapOverrun)
        } else {
            trace!(
                "PDO mapping validation successful for index {:#06X}. Required: {} bytes, Hard Limit: {}",
                index, required_bytes, payload_limit_bytes
            );
            Ok(())
        }
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
                    1 => true,                               // Save All
                    2 => (0x1000..=0x1FFF).contains(&index), // Save Communication
                    3 => (0x6000..=0x9FFF).contains(&index), // Save Application
                    _ => false, // Other sub-indices are manufacturer-specific
                };

                if should_save {
                    if let Some(access) = entry.access {
                        if matches!(
                            access,
                            AccessType::ReadWriteStore | AccessType::WriteOnlyStore
                        ) {
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
                category: Category::Mandatory,
                access: Some(AccessType::ReadWrite),
                default_value: None,
                value_range: None,
                pdo_mapping: None,
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
                category: Category::Mandatory,
                access: None, // Access is per-element for arrays
                default_value: None,
                value_range: None,
                pdo_mapping: None,
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
                category: Category::Mandatory,
                access: None,
                default_value: None,
                value_range: None,
                pdo_mapping: None,
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
                category: Category::Mandatory,
                access: Some(AccessType::ReadOnly),
                default_value: None,
                value_range: None,
                pdo_mapping: None,
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
                    category: Category::Optional,
                    access: Some(AccessType::ReadWriteStore),
                    default_value: None,
                    value_range: None,
                    pdo_mapping: None,
                },
            );

            od.write(0x1010, 3, ObjectValue::VisibleString("save".to_string())) // Save Application Params
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
        let mut od = ObjectDictionary::new(Some(&mut storage));
        assert!(!od.storage.as_ref().unwrap().restore_defaults_requested());
        od.write(0x1011, 1, ObjectValue::VisibleString("load".to_string()))
            .unwrap();
        // After the write, the storage inside od is now borrowed mutably.
        // To check its state, we must borrow it again immutably.
        assert!(od.storage.as_ref().unwrap().restore_defaults_requested());
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
                category: Category::Optional,
                access: Some(AccessType::ReadWriteStore),
                default_value: None,
                value_range: None,
                pdo_mapping: None,
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

        {
            let mut od = ObjectDictionary::new(Some(&mut storage));
            od.insert(
                0x6000,
                ObjectEntry {
                    object: Object::Variable(ObjectValue::Unsigned32(0)), // Firmware default
                    name: "StorableVar",
                    category: Category::Optional,
                    access: Some(AccessType::ReadWriteStore),
                    default_value: None,
                    value_range: None,
                    pdo_mapping: None,
                },
            );
            od.init().unwrap();
            assert_eq!(od.read_u32(0x6000, 0).unwrap(), 0); // Back to default
        }

        assert!(storage.clear_called);
        assert!(!storage.save_called);
        assert!(!storage.restore_requested);
    }

    #[test]
    fn test_pdo_mapping_validation_success() {
        let mut od = ObjectDictionary::new(None);
        // IsochrTxMaxPayload = 100 bytes, PresActPayloadLimit = 40 bytes
        od.insert(0x1F98, ObjectEntry { object: Object::Record(vec![ObjectValue::Unsigned16(100), ObjectValue::Unsigned16(0), ObjectValue::Unsigned32(0), ObjectValue::Unsigned16(0), ObjectValue::Unsigned16(40)]), name: "NMT_CycleTiming_REC", category: Category::Mandatory, access: None, default_value: None, value_range: None, pdo_mapping: None });
        // Mapping for PRes (TPDO 0x1A00) requiring 6 bytes. 6 < 100 (hard limit) -> OK. 6 < 40 (actual limit) -> OK at runtime.
        let mapping1 = PdoMappingEntry { index: 0x6000, sub_index: 1, offset_bits: 0, length_bits: 8 }; // 1 byte at offset 0
        let mapping2 = PdoMappingEntry { index: 0x6001, sub_index: 0, offset_bits: 16, length_bits: 32 }; // 4 bytes at offset 2. Total size = 2+4=6 bytes.
        od.insert(0x1A00, ObjectEntry { object: Object::Array(vec![ObjectValue::Unsigned64(mapping1.to_u64()), ObjectValue::Unsigned64(mapping2.to_u64())]), name: "PDO_TxMappParam_00h_AU64", category: Category::Mandatory, access: None, default_value: None, value_range: None, pdo_mapping: None });

        // Try to enable a mapping with 2 entries. Total size is 6 bytes, which is < 100 hard limit.
        let result = od.write(0x1A00, 0, ObjectValue::Unsigned8(2));
        assert!(result.is_ok());
    }

    #[test]
    fn test_pdo_mapping_validation_failure_hard_limit() {
        let mut od = ObjectDictionary::new(None);
        // IsochrTxMaxPayload = 10 bytes (hard limit)
        od.insert(0x1F98, ObjectEntry { object: Object::Record(vec![ObjectValue::Unsigned16(10), ObjectValue::Unsigned16(0), ObjectValue::Unsigned32(0), ObjectValue::Unsigned16(0), ObjectValue::Unsigned16(40)]), name: "NMT_CycleTiming_REC", category: Category::Mandatory, access: None, default_value: None, value_range: None, pdo_mapping: None });
        // Mapping for PRes (TPDO 0x1A00) requiring 12 bytes.
        let mapping1 = PdoMappingEntry { index: 0x6000, sub_index: 1, offset_bits: 0, length_bits: 64 }; // 8 bytes at offset 0
        let mapping2 = PdoMappingEntry { index: 0x6001, sub_index: 0, offset_bits: 64, length_bits: 32 }; // 4 bytes at offset 8. Total size = 8+4=12 bytes.
        od.insert(0x1A00, ObjectEntry { object: Object::Array(vec![ObjectValue::Unsigned64(mapping1.to_u64()), ObjectValue::Unsigned64(mapping2.to_u64())]), name: "PDO_TxMappParam_00h_AU64", category: Category::Mandatory, access: None, default_value: None, value_range: None, pdo_mapping: None });

        // Try to enable a mapping with 2 entries. Total size is 12 bytes, which is > 10 hard limit.
        let result = od.write(0x1A00, 0, ObjectValue::Unsigned8(2));
        assert!(matches!(result, Err(PowerlinkError::PdoMapOverrun)));
    }

    #[test]
    fn test_pdo_mapping_validation_success_soft_limit() {
        let mut od = ObjectDictionary::new(None);
        // IsochrTxMaxPayload = 100 bytes (hard limit), PresActPayloadLimit = 10 bytes (soft limit)
        od.insert(0x1F98, ObjectEntry { object: Object::Record(vec![ObjectValue::Unsigned16(100), ObjectValue::Unsigned16(0), ObjectValue::Unsigned32(0), ObjectValue::Unsigned16(0), ObjectValue::Unsigned16(10)]), name: "NMT_CycleTiming_REC", category: Category::Mandatory, access: None, default_value: None, value_range: None, pdo_mapping: None });
        // Mapping for PRes (TPDO 0x1A00) requiring 12 bytes.
        let mapping1 = PdoMappingEntry { index: 0x6000, sub_index: 1, offset_bits: 0, length_bits: 64 }; // 8 bytes at offset 0
        let mapping2 = PdoMappingEntry { index: 0x6001, sub_index: 0, offset_bits: 64, length_bits: 32 }; // 4 bytes at offset 8. Total size = 8+4=12 bytes.
        od.insert(0x1A00, ObjectEntry { object: Object::Array(vec![ObjectValue::Unsigned64(mapping1.to_u64()), ObjectValue::Unsigned64(mapping2.to_u64())]), name: "PDO_TxMappParam_00h_AU64", category: Category::Mandatory, access: None, default_value: None, value_range: None, pdo_mapping: None });

        // Try to enable a mapping with 2 entries. Total size is 12 bytes.
        // 12 > 10 (soft limit) but 12 < 100 (hard limit).
        // The configuration should be ACCEPTED.
        let result = od.write(0x1A00, 0, ObjectValue::Unsigned8(2));
        assert!(result.is_ok(), "Configuration should be accepted even if it exceeds the soft limit");
    }
}