mod entry;
mod predefined;
mod value;

pub use entry::{AccessType, Category, Object, ObjectEntry, PdoMapping, ValueRange};
pub use value::ObjectValue;

use crate::hal::ObjectDictionaryStorage;
use crate::{NodeId, PowerlinkError, pdo::PdoMappingEntry};
use alloc::{borrow::Cow, collections::BTreeMap, vec::Vec};
use core::fmt;
use log::{error, trace};

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
                        None // Variables only have sub-index 0
                    }
                }
                Object::Array(values) | Object::Record(values) => {
                    if sub_index == 0 {
                        // Sub-index 0 of Array/Record always returns the number of elements (excluding sub-index 0 itself)
                        // Note: The length is derived from the *current* state of the Vec.
                        // If writing to sub-index 0 modifies the Vec size later, this value might become stale
                        // if not re-read. For PDO mapping, this should reflect the number of *configured* entries.
                        Some(Cow::Owned(ObjectValue::Unsigned8(values.len() as u8)))
                    } else {
                        // Access elements using sub_index - 1
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
        trace!("Attempting OD write: {:#06X}/{}, Value: {:?}", index, sub_index, value);
        // Special case for Store Parameters command (1010h).
        if index == 0x1010 {
            if let ObjectValue::VisibleString(s) = &value {
                // Allow "save" string for sub-indices > 0
                if sub_index > 0 && s == "save" {
                    return self.store_parameters(sub_index);
                }
            }
            // Reject if sub-index is 0 or signature is wrong
            error!("Invalid signature or sub-index for Store Parameters (1010h)");
            return Err(PowerlinkError::StorageError(
                "Invalid signature or sub-index for Store Parameters",
            ));
        }

        // Special case for Restore Default Parameters command (1011h).
        if index == 0x1011 {
            if let ObjectValue::VisibleString(s) = &value {
                 // Allow "load" string for sub-indices > 0
                if sub_index > 0 && s == "load" {
                    return self.restore_defaults(sub_index);
                }
            }
             // Reject if sub-index is 0 or signature is wrong
            error!("Invalid signature or sub-index for Restore Defaults (1011h)");
            return Err(PowerlinkError::StorageError(
                "Invalid signature or sub-index for Restore Defaults",
            ));
        }

        // --- Special validation for PDO mapping ---
        // Check if writing to sub-index 0 of a PDO mapping object (0x16xx or 0x1Axx).
        let is_pdo_mapping_index0 = sub_index == 0
            && ((0x1600..=0x16FF).contains(&index) || (0x1A00..=0x1AFF).contains(&index));

        if is_pdo_mapping_index0 {
            if let ObjectValue::Unsigned8(new_num_entries) = value {
                // Validate the mapping size *before* attempting the internal write.
                self.validate_pdo_mapping(index, new_num_entries)?;
                 trace!("PDO mapping validation successful for {:#06X}, enabling {} entries.", index, new_num_entries);
                // Proceed to write_internal, but bypass normal access checks and the sub-index 0 check
                 return self.write_internal(index, sub_index, value, false);
            } else {
                // NumberOfEntries must always be a U8 value.
                error!("Type mismatch writing to PDO mapping {:#06X}/0: Expected U8.", index);
                return Err(PowerlinkError::TypeMismatch);
            }
        }

        // --- Normal write for other objects/sub-indices ---
        self.write_internal(index, sub_index, value, true)
    }

    /// Finds an object by its string name. Returns the index and sub-index if found.
    /// Note: This performs a linear search and may be slow.
    pub fn find_by_name(&self, name: &str) -> Option<(u16, u8)> {
        for (&index, entry) in &self.entries {
            // Check the main object name
            if entry.name == name {
                // If it's a variable, sub-index is always 0
                if let Object::Variable(_) = entry.object {
                    return Some((index, 0));
                }
                // For records/arrays, finding by main name isn't well-defined without a sub-index name.
                // This basic implementation will just match the main object name and return sub-index 0.
                return Some((index, 0));
            }
            // TODO: A more advanced implementation could search sub-index names if they were stored.
        }
        None
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
            .ok_or(PowerlinkError::ObjectNotFound) // Use map_or
            .and_then(|entry| { // Use and_then for cleaner error propagation
                if check_access {
                    if let Some(access) = entry.access {
                        if matches!(access, AccessType::ReadOnly | AccessType::Constant) {
                            error!("Attempted write to read-only object {:#06X}", index);
                            return Err(PowerlinkError::StorageError("Object is read-only"));
                        }
                    }
                     // If access is None, we assume ReadWrite for Arrays/Records unless sub-index logic dictates otherwise.
                     // The spec doesn't clearly define access for the whole Array/Record vs. sub-indices.
                     // We'll primarily rely on sub-index specific checks.
                }

                 // Special handling for PDO mapping NumberOfEntries write (sub-index 0)
                let is_pdo_mapping_index0 = sub_index == 0
                    && ((0x1600..=0x16FF).contains(&index) || (0x1A00..=0x1AFF).contains(&index));

                match &mut entry.object {
                    Object::Variable(v) => {
                        if sub_index == 0 {
                            // Check type compatibility before writing
                            if core::mem::discriminant(v) != core::mem::discriminant(&value) {
                                error!("Type mismatch writing Variable {:#06X}/0. Expected {:?}, got {:?}", index, v, value);
                                return Err(PowerlinkError::TypeMismatch);
                            }
                            *v = value;
                            Ok(())
                        } else {
                            Err(PowerlinkError::SubObjectNotFound)
                        }
                    }
                    Object::Array(values) | Object::Record(values) => {
                        // FIX: Allow writing to sub-index 0 *only* for PDO mapping objects (NumberOfEntries)
                        if sub_index == 0 {
                            if is_pdo_mapping_index0 {
                                // This write enables/disables the mapping based on the value (NumberOfEntries).
                                // The actual mapping entries (sub-index > 0) are assumed to be written beforehand.
                                // We don't modify the `values` Vec here; validation already passed in `write`.
                                // We just need to allow this specific write operation to succeed logically.
                                // A U8 value is expected here, checked in the `write` function.
                                trace!("Allowing write to sub-index 0 for PDO mapping object {:#06X}", index);
                                Ok(())
                            } else {
                                // For non-PDO Array/Record, writing to sub-index 0 is generally forbidden.
                                error!("Attempted write to sub-index 0 of non-PDO Array/Record {:#06X}", index);
                                Err(PowerlinkError::StorageError(
                                    "Cannot write to sub-index 0 of standard Array/Record",
                                ))
                            }
                        } else if let Some(v) = values.get_mut(sub_index as usize - 1) {
                             // Check type compatibility before writing sub-index > 0
                             if core::mem::discriminant(v) != core::mem::discriminant(&value) {
                                error!("Type mismatch writing {:#06X}/{}. Expected {:?}, got {:?}", index, sub_index, v, value);
                                return Err(PowerlinkError::TypeMismatch);
                             }
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
             trace!("PDO mapping {:#06X} deactivated (0 entries). Validation skipped.", index);
            return Ok(()); // Deactivating a mapping is always valid.
        }

        let is_tpdo = (0x1A00..=0x1AFF).contains(&index);
        let is_rpdo = (0x1600..=0x16FF).contains(&index);
        if !is_tpdo && !is_rpdo {
             // Should not happen if called correctly from `write`
             error!("validate_pdo_mapping called for non-PDO index {:#06X}", index);
             return Ok(());
        }

        // --- 1. Determine the HARD payload size limit for this PDO channel ---
        // Per spec 6.4.8.2, this check is against the max buffer size (0x1F98).
        let payload_limit_bytes = if is_tpdo {
            // TPDOs (PRes on CN, PReq on MN) checked against IsochrTxMaxPayload_U16 (0x1F98/1).
            self.read_u16(0x1F98, 1).unwrap_or(1490) as usize
        } else { // is_rpdo
            // RPDOs (PReq on CN, PRes on MN) checked against IsochrRxMaxPayload_U16 (0x1F98/2).
            self.read_u16(0x1F98, 2).unwrap_or(1490) as usize
        };

        // --- 2. Calculate the required size from the existing mapping entries ---
        // The spec implies mapping entries (sub > 0) are written *before* NumberOfEntries (sub 0) is set.
        let mut max_bits_required: u32 = 0;
        // Read the *current* state of the mapping object array.
        if let Some(Object::Array(entries)) = self.read_object(index) {
            // Iterate up to the *new* number of entries being proposed.
            for i in 0..(new_num_entries as usize) {
                // .get(i) accesses the elements written to sub-indices 1, 2, ...
                if let Some(ObjectValue::Unsigned64(raw_mapping)) = entries.get(i) {
                    let entry = PdoMappingEntry::from_u64(*raw_mapping);
                     // Calculate the end position (offset + length) in bits.
                    let end_pos_bits = entry.offset_bits as u32 + entry.length_bits as u32;
                     // Keep track of the highest bit position reached.
                    max_bits_required = max_bits_required.max(end_pos_bits);
                } else {
                    // This indicates a configuration error: trying to enable more entries than have been written/exist.
                    error!(
                        "PDO mapping validation error for {:#06X}: Trying to enable {} entries, but entry {} is missing.",
                        index, new_num_entries, i + 1
                    );
                    return Err(PowerlinkError::ValidationError("Incomplete PDO mapping configuration: Missing entries"));
                }
            }
        } else {
             // This should not happen if called correctly, as the object must exist.
             error!("PDO mapping validation error: Object {:#06X} not found or not an Array.", index);
            return Err(PowerlinkError::ObjectNotFound);
        }

        // Convert the highest bit position to required bytes, rounding up.
        let required_bytes = (max_bits_required + 7) / 8;

        // --- 3. Compare required size against the HARD limit and return result ---
        if required_bytes as usize > payload_limit_bytes {
            error!(
                "PDO mapping validation failed for index {:#06X}. Required size: {} bytes, Hard Limit: {} bytes. [E_PDO_MAP_OVERRUN]",
                index, required_bytes, payload_limit_bytes
            );
            Err(PowerlinkError::PdoMapOverrun) // Use the specific error
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
             error!("Attempted to store parameters with invalid sub-index 0.");
            return Err(PowerlinkError::StorageError("Cannot save to sub-index 0"));
        }
        if let Some(s) = &mut self.storage {
            trace!("Storing parameters for sub-index {}", list_to_save);
            let mut storable_params = BTreeMap::new();
            for (&index, entry) in &self.entries {
                // Determine if this object's group (Comm, App, etc.) should be saved based on list_to_save.
                let should_save = match list_to_save {
                    1 => true,                               // Save All
                    2 => (0x1000..=0x1FFF).contains(&index), // Save Communication
                    3 => (0x6000..=0x9FFF).contains(&index), // Save Application
                    _ => false, // Other sub-indices are manufacturer-specific (ignore for now)
                };

                if should_save {
                    // Check if the entry itself has a storable access type
                     if let Some(access) = entry.access {
                        if matches!(
                            access,
                            AccessType::ReadWriteStore | AccessType::WriteOnlyStore
                        ) {
                             // Extract values based on object type
                            match &entry.object {
                                Object::Variable(val) => {
                                    // Store Variable at sub-index 0
                                    storable_params.insert((index, 0), val.clone());
                                }
                                Object::Record(vals) | Object::Array(vals) => {
                                     // Store Array/Record elements at sub-indices 1..N
                                    for (i, val) in vals.iter().enumerate() {
                                        storable_params.insert((index, (i + 1) as u8), val.clone());
                                    }
                                }
                            }
                        }
                    }
                    // TODO: Handle storing sub-indices of Records/Arrays individually if they have different access types?
                    // The current logic assumes the main entry's access applies to all parts.
                }
            }
            if storable_params.is_empty() {
                 trace!("No storable parameters found for sub-index {}", list_to_save);
             } else {
                 trace!("Saving {} parameters.", storable_params.len());
             }
            s.save(&storable_params)
        } else {
             error!("Store parameters failed: No storage backend configured.");
            Err(PowerlinkError::StorageError(
                "No storage backend configured",
            ))
        }
    }

    /// Tells the storage backend to set a flag to restore defaults on the next boot.
    /// The actual data clearing happens at startup during `init()`.
    fn restore_defaults(&mut self, list_to_restore: u8) -> Result<(), PowerlinkError> {
        if list_to_restore == 0 {
            error!("Attempted to restore defaults with invalid sub-index 0.");
            return Err(PowerlinkError::StorageError(
                "Cannot restore from sub-index 0",
            ));
        }
        if let Some(s) = &mut self.storage {
            // For now, any valid "load" command flags a full restore.
            // A full implementation could pass `list_to_restore` to the storage backend
            // to potentially allow partial restores, although the spec isn't explicit on this.
             trace!("Requesting restore defaults for sub-index {}", list_to_restore);
            s.request_restore_defaults()
        } else {
             error!("Restore defaults failed: No storage backend configured.");
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
         // Test reading non-existent sub-index
         assert!(od.read(0x1006, 1).is_none());
    }

    #[test]
    fn test_read_write_array_element() {
        let mut od = ObjectDictionary::new(None);
        od.insert(
            0x2000,
            ObjectEntry {
                 // Initialize with one element for sub-index 1
                object: Object::Array(vec![ObjectValue::Unsigned16(100)]),
                name: "TestArray",
                category: Category::Mandatory,
                access: Some(AccessType::ReadWrite), // Assume ReadWrite for elements if main access is RW/None
                default_value: None,
                value_range: None,
                pdo_mapping: None,
            },
        );

        // Write to existing sub-index 1
        od.write(0x2000, 1, ObjectValue::Unsigned16(999)).unwrap();
        let value = od.read(0x2000, 1).unwrap();
        assert_eq!(*value, ObjectValue::Unsigned16(999));

         // Test writing to non-existent sub-index
         assert!(matches!(od.write(0x2000, 2, ObjectValue::Unsigned16(111)), Err(PowerlinkError::SubObjectNotFound)));
         // Test writing wrong type
         assert!(matches!(od.write(0x2000, 1, ObjectValue::Unsigned8(5)), Err(PowerlinkError::TypeMismatch)));
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

        // Read sub-index 0, should return the length (2) as an owned U8
        let value = od.read(0x2000, 0).unwrap();
        assert_eq!(*value, ObjectValue::Unsigned8(2));
        // Check that it's an owned value, not borrowed from the internal vec
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
        // Verify the value hasn't changed
        assert_eq!(*od.read(0x1008, 0).unwrap(), ObjectValue::Unsigned8(10));
    }

    #[test]
    fn test_save_command() {
        let mut storage = MockStorage::new();
        {
            let mut od = ObjectDictionary::new(Some(&mut storage));
            od.insert(
                0x6000, // Application object
                ObjectEntry {
                    object: Object::Variable(ObjectValue::Unsigned32(123)),
                    name: "StorableAppVar",
                    category: Category::Optional,
                    access: Some(AccessType::ReadWriteStore), // Storable
                    default_value: None,
                    value_range: None,
                    pdo_mapping: None,
                },
            );
             od.insert(
                0x1800, // Communication object
                ObjectEntry {
                    object: Object::Record(vec![ObjectValue::Unsigned8(10), ObjectValue::Unsigned8(1)]),
                    name: "StorableCommVar",
                    category: Category::Optional,
                    access: Some(AccessType::ReadWriteStore), // Storable
                    default_value: None,
                    value_range: None,
                    pdo_mapping: None,
                },
            );
             od.insert(
                0x7000, // Another Application object
                ObjectEntry {
                    object: Object::Variable(ObjectValue::Integer16(-5)),
                    name: "NonStorableAppVar",
                    category: Category::Optional,
                    access: Some(AccessType::ReadWrite), // Not storable
                    default_value: None,
                    value_range: None,
                    pdo_mapping: None,
                },
            );

            // Save Application Params (sub-index 3)
            od.write(0x1010, 3, ObjectValue::VisibleString("save".to_string()))
                .unwrap();
        } // od goes out of scope here, storage can be checked

        assert!(storage.save_called);
        // Check that only the storable application var (0x6000) was saved
        assert_eq!(storage.saved_data.len(), 1);
        assert_eq!(
            storage.saved_data.get(&(0x6000, 0)),
            Some(&ObjectValue::Unsigned32(123))
        );
         assert!(storage.saved_data.get(&(0x1800, 1)).is_none()); // Comm var not saved
         assert!(storage.saved_data.get(&(0x7000, 0)).is_none()); // Non-storable var not saved
    }

    #[test]
    fn test_restore_defaults_command_flags_for_reboot() {
        let mut storage = MockStorage::new();
        { // Scope for mutable borrow of storage by OD
            let mut od = ObjectDictionary::new(Some(&mut storage));
            assert!(!od.storage.as_ref().unwrap().restore_defaults_requested());

            // Write "load" signature to sub-index 1 (Restore All)
            od.write(0x1011, 1, ObjectValue::VisibleString("load".to_string()))
                .unwrap();

            // Check that the flag is set within the storage backend
            assert!(od.storage.as_ref().unwrap().restore_defaults_requested());

             // Writing wrong signature should fail
             assert!(od.write(0x1011, 1, ObjectValue::VisibleString("loads".to_string())).is_err());
             // Writing to sub-index 0 should fail
             assert!(od.write(0x1011, 0, ObjectValue::VisibleString("load".to_string())).is_err());
        } // od goes out of scope

        // Verify flag remains set after OD is dropped
        assert!(storage.restore_requested);
    }


    #[test]
    fn test_loading_from_storage_on_init() {
        let mut storage = MockStorage::new();
        // Pre-populate storage with a value
        storage
            .saved_data
            .insert((0x6000, 0), ObjectValue::Unsigned32(999));

        let mut od = ObjectDictionary::new(Some(&mut storage));
        // Add the corresponding object entry with a different initial value
        od.insert(
            0x6000,
            ObjectEntry {
                object: Object::Variable(ObjectValue::Unsigned32(0)), // Firmware default
                name: "StorableVar",
                category: Category::Optional,
                access: Some(AccessType::ReadWriteStore), // Important: Must be storable to be loaded
                default_value: None,
                value_range: None,
                pdo_mapping: None,
            },
        );

        od.init().unwrap(); // init() should call load()

        // Check that the value loaded from storage overwrote the initial value
        assert_eq!(od.read_u32(0x6000, 0).unwrap(), 999);
         assert!(storage.load_called);
    }

    #[test]
    fn test_init_restores_defaults_if_flagged() {
        let mut storage = MockStorage::new();
        // Pre-populate storage
        storage
            .saved_data
            .insert((0x6000, 0), ObjectValue::Unsigned32(999));
        // Set the restore flag
        storage.restore_requested = true;

        { // Scope for OD borrow
            let mut od = ObjectDictionary::new(Some(&mut storage));
            od.insert(
                0x6000,
                ObjectEntry {
                    object: Object::Variable(ObjectValue::Unsigned32(0)), // Firmware default
                    name: "StorableVar",
                    category: Category::Optional,
                    access: Some(AccessType::ReadWriteStore),
                    default_value: Some(ObjectValue::Unsigned32(0)), // Explicit default
                    value_range: None,
                    pdo_mapping: None,
                },
            );

            od.init().unwrap(); // init() detects flag, clears storage, skips load

            // Check that the value is the firmware default, not the stored value
            assert_eq!(od.read_u32(0x6000, 0).unwrap(), 0);
        } // od goes out of scope

        assert!(storage.clear_called); // Storage should have been cleared
        assert!(!storage.load_called); // Load should have been skipped
        assert!(!storage.restore_requested); // Flag should have been cleared
    }

    #[test]
    fn test_pdo_mapping_validation_success() {
        let mut od = ObjectDictionary::new(None);
        // IsochrTxMaxPayload (Hard Limit) = 100 bytes (0x1F98/1)
        // PresActPayloadLimit (Soft Limit) = 40 bytes (0x1F98/5)
        od.insert(0x1F98, ObjectEntry { object: Object::Record(vec![ObjectValue::Unsigned16(100), ObjectValue::Unsigned16(0), ObjectValue::Unsigned32(0), ObjectValue::Unsigned16(0), ObjectValue::Unsigned16(40)]), name: "NMT_CycleTiming_REC", category: Category::Mandatory, access: None, default_value: None, value_range: None, pdo_mapping: None });
        // Mapping for PRes (TPDO 0x1A00) requiring 6 bytes total.
        let mapping1 = PdoMappingEntry { index: 0x6000, sub_index: 1, offset_bits: 0, length_bits: 8 }; // 1 byte @ offset 0
        let mapping2 = PdoMappingEntry { index: 0x6001, sub_index: 0, offset_bits: 16, length_bits: 32 }; // 4 bytes @ offset 2. Max bit = 16+32=48. Required bytes = ceil(48/8) = 6.
        od.insert(0x1A00, ObjectEntry { object: Object::Array(vec![ObjectValue::Unsigned64(mapping1.to_u64()), ObjectValue::Unsigned64(mapping2.to_u64())]), name: "PDO_TxMappParam_00h_AU64", category: Category::Mandatory, access: Some(AccessType::ReadWriteStore), default_value: None, value_range: None, pdo_mapping: None }); // Use RW access

        // Try to enable the mapping with 2 entries.
        // Required size (6 bytes) < Hard Limit (100 bytes). Should succeed.
        let result = od.write(0x1A00, 0, ObjectValue::Unsigned8(2));
        // FIX: Check if the result is Ok.
        assert!(result.is_ok(), "Validation failed unexpectedly: {:?}", result.err());
    }

    #[test]
    fn test_pdo_mapping_validation_failure_hard_limit() {
        let mut od = ObjectDictionary::new(None);
        // IsochrTxMaxPayload (Hard limit) = 10 bytes
        od.insert(0x1F98, ObjectEntry { object: Object::Record(vec![ObjectValue::Unsigned16(10), ObjectValue::Unsigned16(0), ObjectValue::Unsigned32(0), ObjectValue::Unsigned16(0), ObjectValue::Unsigned16(40)]), name: "NMT_CycleTiming_REC", category: Category::Mandatory, access: None, default_value: None, value_range: None, pdo_mapping: None });
        // Mapping for PRes (TPDO 0x1A00) requiring 12 bytes. Max bit = 64+32=96. Bytes = 12.
        let mapping1 = PdoMappingEntry { index: 0x6000, sub_index: 1, offset_bits: 0, length_bits: 64 }; // 8 bytes @ offset 0
        let mapping2 = PdoMappingEntry { index: 0x6001, sub_index: 0, offset_bits: 64, length_bits: 32 }; // 4 bytes @ offset 8.
        od.insert(0x1A00, ObjectEntry { object: Object::Array(vec![ObjectValue::Unsigned64(mapping1.to_u64()), ObjectValue::Unsigned64(mapping2.to_u64())]), name: "PDO_TxMappParam_00h_AU64", category: Category::Mandatory, access: Some(AccessType::ReadWriteStore), default_value: None, value_range: None, pdo_mapping: None }); // Use RW access

        // Try to enable mapping with 2 entries.
        // Required size (12 bytes) > Hard Limit (10 bytes). Should fail.
        let result = od.write(0x1A00, 0, ObjectValue::Unsigned8(2));
        assert!(matches!(result, Err(PowerlinkError::PdoMapOverrun)));
    }

    #[test]
    fn test_pdo_mapping_validation_success_soft_limit() {
        let mut od = ObjectDictionary::new(None);
        // IsochrTxMaxPayload = 100 bytes (hard limit), PresActPayloadLimit = 10 bytes (soft limit, irrelevant for validation)
        od.insert(0x1F98, ObjectEntry { object: Object::Record(vec![ObjectValue::Unsigned16(100), ObjectValue::Unsigned16(0), ObjectValue::Unsigned32(0), ObjectValue::Unsigned16(0), ObjectValue::Unsigned16(10)]), name: "NMT_CycleTiming_REC", category: Category::Mandatory, access: None, default_value: None, value_range: None, pdo_mapping: None });
        // Mapping for PRes (TPDO 0x1A00) requiring 12 bytes.
        let mapping1 = PdoMappingEntry { index: 0x6000, sub_index: 1, offset_bits: 0, length_bits: 64 }; // 8 bytes @ offset 0
        let mapping2 = PdoMappingEntry { index: 0x6001, sub_index: 0, offset_bits: 64, length_bits: 32 }; // 4 bytes @ offset 8. Max bit = 96. Bytes = 12.
        od.insert(0x1A00, ObjectEntry { object: Object::Array(vec![ObjectValue::Unsigned64(mapping1.to_u64()), ObjectValue::Unsigned64(mapping2.to_u64())]), name: "PDO_TxMappParam_00h_AU64", category: Category::Mandatory, access: Some(AccessType::ReadWriteStore), default_value: None, value_range: None, pdo_mapping: None }); // Use RW access

        // Try to enable mapping with 2 entries.
        // Required size (12 bytes) > Soft Limit (10 bytes) BUT Required size < Hard Limit (100 bytes).
        // The configuration validation should ACCEPT this based on the HARD limit check.
        let result = od.write(0x1A00, 0, ObjectValue::Unsigned8(2));
        // FIX: Check if the result is Ok.
        assert!(result.is_ok(), "Configuration should be accepted even if it exceeds the soft limit, as long as it's within the hard limit. Error: {:?}", result.err());
    }
}