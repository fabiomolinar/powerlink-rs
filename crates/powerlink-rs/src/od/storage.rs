// crates/powerlink-rs/src/od/storage.rs
use super::{predefined, ObjectDictionary};
use crate::PowerlinkError;

/// Initialises the Object Dictionary.
/// 1. Populates mandatory communication profile objects.
/// 2. Checks if a "Restore Defaults" command was flagged in storage.
/// 3. If so, clears storage and proceeds with firmware defaults.
/// 4. If not, loads all stored parameters from the backend.
pub fn init(od: &mut ObjectDictionary) -> Result<(), PowerlinkError> {
    let mut restore_defaults = false;
    if let Some(s) = &mut od.storage {
        if s.restore_defaults_requested() {
            restore_defaults = true;
            s.clear_restore_defaults_flag()?;
            s.clear()?;
        }
    }

    predefined::populate_protocol_objects(od);

    if !restore_defaults {
        load(od)?;
    }
    Ok(())
}

/// Loads values from the persistent storage backend and overwrites any
/// matching existing entries in the OD.
fn load(od: &mut ObjectDictionary) -> Result<(), PowerlinkError> {
    if let Some(s) = &mut od.storage {
        let stored_params = s.load()?;
        for ((index, sub_index), value) in stored_params {
            // Attempt to write the loaded value. Ignore errors for objects
            // that might exist in storage but not in the current firmware.
            let _ = od.write_internal(index, sub_index, value, false);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hal::ObjectDictionaryStorage;
    use crate::od::{AccessType, Category, Object, ObjectEntry, ObjectValue};
    use alloc::collections::BTreeMap;
    use alloc::vec;

    struct MockStorage {
        saved_data: BTreeMap<(u16, u8), ObjectValue>,
        restore_requested: bool,
        load_called: bool,
        clear_called: bool,
    }
    impl MockStorage {
        fn new() -> Self {
            Self {
                saved_data: BTreeMap::new(),
                restore_requested: false,
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
            _params: &BTreeMap<(u16, u8), ObjectValue>,
        ) -> Result<(), PowerlinkError> {
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
    fn test_loading_from_storage_on_init() {
        let mut storage = MockStorage::new();
        storage
            .saved_data
            .insert((0x6000, 0), ObjectValue::Unsigned32(999));

        let mut od = ObjectDictionary::new(Some(&mut storage));
        od.insert(
            0x6000,
            ObjectEntry {
                object: Object::Variable(ObjectValue::Unsigned32(0)), // Firmware default
                name: "StorableVar",
                category: Category::Optional,
                access: Some(AccessType::ReadWriteStore),
                ..Default::default()
            },
        );

        init(&mut od).unwrap();

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
                    default_value: Some(ObjectValue::Unsigned32(0)),
                    ..Default::default()
                },
            );

            init(&mut od).unwrap();

            assert_eq!(od.read_u32(0x6000, 0).unwrap(), 0);
            assert!(!storage.restore_defaults_requested());
        }
    }
}