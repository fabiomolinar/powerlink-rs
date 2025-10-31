// crates/powerlink-rs/src/od/commands.rs
use super::{AccessType, Object, ObjectDictionary, ObjectValue};
use crate::PowerlinkError;
use crate::hal::ObjectDictionaryStorage;
use alloc::collections::BTreeMap;
use log::{error, trace};

/// Collects all storable parameters and tells the storage backend to save them.
pub fn store_parameters(od: &mut ObjectDictionary, list_to_save: u8) -> Result<(), PowerlinkError> {
    if list_to_save == 0 {
        error!("Attempted to store parameters with invalid sub-index 0.");
        return Err(PowerlinkError::StorageError("Cannot save to sub-index 0"));
    }
    if let Some(s) = &mut od.storage {
        trace!("Storing parameters for sub-index {}", list_to_save);
        let mut storable_params = BTreeMap::new();
        for (&index, entry) in &od.entries {
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
            }
        }
        if storable_params.is_empty() {
            trace!(
                "No storable parameters found for sub-index {}",
                list_to_save
            );
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
pub fn restore_defaults(
    od: &mut ObjectDictionary,
    list_to_restore: u8,
) -> Result<(), PowerlinkError> {
    if list_to_restore == 0 {
        error!("Attempted to restore defaults with invalid sub-index 0.");
        return Err(PowerlinkError::StorageError(
            "Cannot restore from sub-index 0",
        ));
    }
    if let Some(s) = &mut od.storage {
        trace!(
            "Requesting restore defaults for sub-index {}",
            list_to_restore
        );
        s.request_restore_defaults()
    } else {
        error!("Restore defaults failed: No storage backend configured.");
        Err(PowerlinkError::StorageError(
            "No storage backend configured",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::od::{Category, ObjectEntry};
    use alloc::string::ToString;
    use alloc::vec;

    struct MockStorage {
        saved_data: BTreeMap<(u16, u8), ObjectValue>,
        restore_requested: bool,
        save_called: bool,
    }
    impl MockStorage {
        fn new() -> Self {
            Self {
                saved_data: BTreeMap::new(),
                restore_requested: false,
                save_called: false,
            }
        }
    }
    impl ObjectDictionaryStorage for MockStorage {
        fn load(&mut self) -> Result<BTreeMap<(u16, u8), ObjectValue>, PowerlinkError> {
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
    fn test_save_command() {
        let mut storage = MockStorage::new();
        let mut od = ObjectDictionary::new(Some(&mut storage));
        od.insert(
            0x6000, // Application object
            ObjectEntry {
                object: Object::Variable(ObjectValue::Unsigned32(123)),
                name: "StorableAppVar",
                category: Category::Optional,
                access: Some(AccessType::ReadWriteStore), // Storable
                ..Default::default()
            },
        );
        od.insert(
            0x1800, // Communication object
            ObjectEntry {
                object: Object::Record(vec![ObjectValue::Unsigned8(10), ObjectValue::Unsigned8(1)]),
                name: "StorableCommVar",
                category: Category::Optional,
                access: Some(AccessType::ReadWriteStore), // Storable
                ..Default::default()
            },
        );
        od.insert(
            0x7000, // Another Application object
            ObjectEntry {
                object: Object::Variable(ObjectValue::Integer16(-5)),
                name: "NonStorableAppVar",
                category: Category::Optional,
                access: Some(AccessType::ReadWrite), // Not storable
                ..Default::default()
            },
        );

        // Directly test the store_parameters function for Application Params (sub-index 3)
        store_parameters(&mut od, 3).unwrap();

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
        let mut od = ObjectDictionary::new(Some(&mut storage));
        assert!(!od.storage.as_ref().unwrap().restore_defaults_requested());

        // Directly test the restore_defaults function
        restore_defaults(&mut od, 1).unwrap();

        assert!(od.storage.as_ref().unwrap().restore_defaults_requested());

        // Test invalid sub-index
        assert!(restore_defaults(&mut od, 0).is_err());
    }
}
