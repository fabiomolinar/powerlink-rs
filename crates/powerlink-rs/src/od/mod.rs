// crates/powerlink-rs/src/od/mod.rs
mod commands;
pub mod constants;
pub mod error_history;
mod entry;
mod pdo_validator;
mod predefined;
mod storage;
pub mod utils;
mod value;

pub use entry::{AccessType, Category, Object, ObjectEntry, PdoMapping, ValueRange};
pub use value::ObjectValue;

use crate::hal::ObjectDictionaryStorage;
use crate::{NodeId, PowerlinkError};
use alloc::{borrow::Cow, collections::BTreeMap, vec::Vec};
use core::fmt;
use log::{error, info, trace, warn};

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

    /// Initialises the Object Dictionary by populating mandatory objects and
    /// loading parameters from the persistent storage backend.
    pub fn init(&mut self) -> Result<(), PowerlinkError> {
        // Delegates strictly to the refactored storage module
        storage::init(self)
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
                // The vector `values` contains data for Sub-Indices 1..N.
                // Index `i` corresponds to Node ID `i + 1`.
                for (i, value) in values.iter().enumerate() {
                    if let ObjectValue::Unsigned32(assignment) = value {
                        // Bit 0: Node exists
                        // Bit 8: Node is isochronous
                        if (assignment & (1 << 0)) != 0 && (assignment & (1 << 8)) == 0 {
                            cn_list.push(NodeId((i + 1) as u8));
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
                        // Sub-index 0 for Array/Record is the entry count.
                        // Per spec 6.2.1.1, this is an UNSIGNED8.
                        // Find the highest valid sub-index (1-254) defined.
                        let count = values.len().min(254) as u8;
                        Some(Cow::Owned(ObjectValue::Unsigned8(count)))
                    } else {
                        // Access actual data at index = sub_index - 1
                        values.get(sub_index as usize - 1).map(Cow::Borrowed)
                    }
                }
            })
    }

    /// Reads an object's enum (`Object::Variable`, `Object::Array`, etc.) by index.
    pub fn read_object(&self, index: u16) -> Option<&Object> {
        self.entries.get(&index).map(|entry| &entry.object)
    }

    // --- Type-Safe Accessors ---
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
        trace!(
            "Attempting OD write: {:#06X}/{}, Value: {:?}",
            index, sub_index, value
        );
        // Handle special command objects.
        if index == 0x1010 {
            if let ObjectValue::VisibleString(s) = &value {
                if sub_index > 0 && s == "save" {
                    return commands::store_parameters(self, sub_index);
                }
            }
            error!("Invalid signature or sub-index for Store Parameters (1010h)");
            return Err(PowerlinkError::StorageError(
                "Invalid signature or sub-index for Store Parameters",
            ));
        }

        if index == 0x1011 {
            if let ObjectValue::VisibleString(s) = &value {
                if sub_index > 0 && s == "load" {
                    return commands::restore_defaults(self, sub_index);
                }
            }
            error!("Invalid signature or sub-index for Restore Defaults (1011h)");
            return Err(PowerlinkError::StorageError(
                "Invalid signature or sub-index for Restore Defaults",
            ));
        }

        // Handle PDO mapping validation.
        let is_pdo_mapping_index0 = sub_index == 0
            && ((0x1600..=0x16FF).contains(&index) || (0x1A00..=0x1AFF).contains(&index));

        if is_pdo_mapping_index0 {
            if let ObjectValue::Unsigned8(new_num_entries) = value {
                pdo_validator::validate_pdo_mapping(self, index, new_num_entries)?;
                trace!(
                    "PDO mapping validation successful for {:#06X}, enabling {} entries.",
                    index, new_num_entries
                );
                // After validation, we still need to write the new entry count.
                // We fall through to write_internal.
            } else {
                error!(
                    "Type mismatch writing to PDO mapping {:#06X}/0: Expected U8.",
                    index
                );
                return Err(PowerlinkError::TypeMismatch);
            }
        }

        // Normal write for other objects/sub-indices.
        self.write_internal(index, sub_index, value, true)
    }

    /// Finds an object by its string name.
    pub fn find_by_name(&self, name: &str) -> Option<(u16, u8)> {
        for (&index, entry) in &self.entries {
            if entry.name == name {
                if let Object::Variable(_) = entry.object {
                    return Some((index, 0));
                }
                return Some((index, 0));
            }
        }
        None
    }
    
    /// Restores "PowerOn" values for all objects within a specific index range.
    ///
    /// "PowerOn" values are defined as:
    /// 1. The value found in persistent storage (if available).
    /// 2. If no stored value exists, the default value from the Object Entry definition.
    /// 3. If no default value exists, the object is left unchanged (or zeroed if necessary).
    ///
    /// This is used by NMT Reset commands (e.g., ResetCommunication, ResetApplication).
    pub fn restore_power_on_values(&mut self, start_index: u16, end_index: u16) {
        info!("Restoring PowerOn values for OD range {:#04X}..={:#04X}", start_index, end_index);

        // 1. Try to load stored parameters from the backend (if available)
        let stored_params = if let Some(s) = &mut self.storage {
            match s.load() {
                Ok(params) => Some(params),
                Err(e) => {
                    warn!("Failed to load stored parameters during reset: {:?}", e);
                    None
                }
            }
        } else {
            None
        };

        // 2. Iterate over all existing entries in the specified range
        for (&index, entry) in self.entries.iter_mut() {
            if index < start_index || index > end_index {
                continue;
            }
            
            // Skip Error History (0x1003) during ResetCommunication (EPSG DS 301, 6.5.10.2)
            if index == 0x1003 {
                continue; 
            }

            // Determine the "PowerOn" value for each sub-index
            match &mut entry.object {
                Object::Variable(val) => {
                    if let Some(stored_val) = stored_params.as_ref().and_then(|p| p.get(&(index, 0))) {
                         *val = stored_val.clone();
                    } else if let Some(default_val) = &entry.default_value {
                        *val = default_val.clone();
                    }
                },
                Object::Array(values) | Object::Record(values) => {
                    for (i, val) in values.iter_mut().enumerate() {
                        let sub_index = (i + 1) as u8;
                        if let Some(stored_val) = stored_params.as_ref().and_then(|p| p.get(&(index, sub_index))) {
                            *val = stored_val.clone();
                        } else if let Some(default_val) = &entry.default_value {
                            // Fallback: If the default value matches the type, apply it.
                            if core::mem::discriminant(val) == core::mem::discriminant(default_val) {
                                 *val = default_val.clone();
                            }
                        }
                    }
                }
            }
        }
    }

    /// Atomically increments an UNSIGNED32 counter in the Object Dictionary.
    pub(super) fn increment_counter(&mut self, index: u16, sub_index: u8) {
        let entry = self.entries.get_mut(&index);

        let Some(entry) = entry else {
            warn!(
                "Attempted to increment non-existent diagnostic counter {:#06X}/{}",
                index, sub_index
            );
            return;
        };

        let object_value = match &mut entry.object {
            Object::Variable(val) if sub_index == 0 => Some(val),
            Object::Record(vals) if sub_index > 0 => vals.get_mut(sub_index as usize - 1),
            _ => None,
        };

        match object_value {
            Some(ObjectValue::Unsigned32(val)) => {
                *val = val.wrapping_add(1);
            }
            _ => {
                warn!(
                    "Attempted to increment diagnostic counter {:#06X}/{} which is not a U32",
                    index, sub_index
                );
            }
        }
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
            .ok_or(PowerlinkError::ObjectNotFound)
            .and_then(|entry| {
                if check_access {
                    if let Some(access) = entry.access {
                        if matches!(access, AccessType::ReadOnly | AccessType::Constant) {
                            error!("Attempted write to read-only object {:#06X}", index);
                            return Err(PowerlinkError::StorageError("Object is read-only"));
                        }
                    }
                }

                let is_pdo_mapping_index0 = sub_index == 0
                    && ((0x1600..=0x16FF).contains(&index)
                        || (0x1A00..=0x1AFF).contains(&index));

                match &mut entry.object {
                    Object::Variable(v) => {
                        if sub_index == 0 {
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
                        if sub_index == 0 {
                            // Writing to sub-index 0 (NumberOfEntries) is allowed for PDO mapping.
                            if is_pdo_mapping_index0 {
                                if let ObjectValue::Unsigned8(new_count) = value {
                                    // The actual value is not stored here.
                                    // The count is derived from the Vec.len() during read.
                                    // This write is only to trigger validation.
                                    trace!(
                                        "Write to PDO mapping {:#06X}/0 processed ({} entries).",
                                        index,
                                        new_count
                                    );
                                    Ok(())
                                } else {
                                    Err(PowerlinkError::TypeMismatch)
                                }
                            } else {
                                error!(
                                    "Attempted write to sub-index 0 of non-PDO Array/Record {:#06X}",
                                    index
                                );
                                Err(PowerlinkError::StorageError(
                                    "Cannot write to sub-index 0 of standard Array/Record",
                                ))
                            }
                        } else if let Some(v) = values.get_mut(sub_index as usize - 1) {
                            if core::mem::discriminant(v) != core::mem::discriminant(&value) {
                                error!(
                                    "Type mismatch writing {:#06X}/{}. Expected {:?}, got {:?}",
                                    index, sub_index, v, value
                                );
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

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
                ..Default::default()
            },
        );

        let value = od.read(0x1006, 0).unwrap();
        assert_eq!(*value, ObjectValue::Unsigned32(12345));
        assert!(od.read(0x1006, 1).is_none());
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
                access: Some(AccessType::ReadWrite),
                ..Default::default()
            },
        );

        od.write(0x2000, 1, ObjectValue::Unsigned16(999)).unwrap();
        let value = od.read(0x2000, 1).unwrap();
        assert_eq!(*value, ObjectValue::Unsigned16(999));

        assert!(matches!(
            od.write(0x2000, 2, ObjectValue::Unsigned16(111)),
            Err(PowerlinkError::SubObjectNotFound)
        ));
        assert!(matches!(
            od.write(0x2000, 1, ObjectValue::Unsigned8(5)),
            Err(PowerlinkError::TypeMismatch)
        ));
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
                ..Default::default()
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
                ..Default::default()
            },
        );

        let result = od.write(0x1008, 0, ObjectValue::Unsigned8(42));
        assert!(result.is_err());
        assert_eq!(*od.read(0x1008, 0).unwrap(), ObjectValue::Unsigned8(10));
    }

    #[test]
    fn test_find_by_name() {
        let mut od = ObjectDictionary::new(None);
        od.insert(
            0x1008,
            ObjectEntry {
                object: Object::Variable(ObjectValue::Unsigned8(10)),
                name: "DeviceName",
                ..Default::default()
            },
        );
        od.insert(
            0x2000,
            ObjectEntry {
                object: Object::Array(vec![ObjectValue::Unsigned16(100)]),
                name: "ErrorFlags",
                ..Default::default()
            },
        );

        assert_eq!(od.find_by_name("DeviceName"), Some((0x1008, 0)));
        assert_eq!(od.find_by_name("ErrorFlags"), Some((0x2000, 0)));
        assert_eq!(od.find_by_name("NonExistent"), None);
    }

    #[test]
    fn test_restore_power_on_values() {
        let mut od = ObjectDictionary::new(None);
        // Add 0x1000 (Communication) and 0x6000 (Application)
        od.insert(0x1000, ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned32(10)),
            default_value: Some(ObjectValue::Unsigned32(99)),
            ..Default::default()
        });
        od.insert(0x6000, ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned32(20)),
            default_value: Some(ObjectValue::Unsigned32(88)),
            ..Default::default()
        });
        
        // Change values
        od.write(0x1000, 0, ObjectValue::Unsigned32(11)).unwrap();
        od.write(0x6000, 0, ObjectValue::Unsigned32(21)).unwrap();

        // Reset Comm only (1000-1FFF)
        od.restore_power_on_values(0x1000, 0x1FFF);

        // 0x1000 should be default (99), 0x6000 should stay 21
        assert_eq!(od.read_u32(0x1000, 0).unwrap(), 99);
        assert_eq!(od.read_u32(0x6000, 0).unwrap(), 21);
    }

    // Default implementation for ObjectEntry to simplify test setup.
    impl Default for ObjectEntry {
        fn default() -> Self {
            Self {
                object: Object::Variable(ObjectValue::Unsigned8(0)),
                name: "Default",
                category: Category::Optional,
                access: None,
                default_value: None,
                value_range: None,
                pdo_mapping: None,
            }
        }
    }
}