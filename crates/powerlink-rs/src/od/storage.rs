// crates/powerlink-rs/src/od/storage.rs
use super::{ObjectDictionary, predefined};
use crate::PowerlinkError;
use log::{info, warn};

/// Initialises the Object Dictionary.
///
/// This function implements a "Layered Initialization" strategy:
/// 1. **Base Layer**: The OD passed to this function contains the "Firmware Defaults" (application-specific values).
/// 2. **Protocol Layer**: Mandatory POWERLINK objects are populated if missing (`predefined::populate_protocol_objects`).
/// 3. **Persistence Layer**:
///    - If a **Restore Defaults** is pending: The storage is wiped, the flag is cleared, and the OD remains at the Firmware/Protocol default state.
///    - If **Normal Boot**: Parameters are loaded from storage and applied as an overlay, overwriting the defaults.
pub fn init(od: &mut ObjectDictionary) -> Result<(), PowerlinkError> {
    // 1. & 2. Ensure the OD has a valid structure (Firmware + Protocol Defaults).
    // This must happen BEFORE loading from storage, so that storage has valid
    // objects to write into.
    predefined::populate_protocol_objects(od);

    // 3. Handle Persistence
    if let Some(s) = &mut od.storage {
        if s.restore_defaults_requested() {
            info!("Restore Defaults requested. Clearing persistent storage.");
            
            // CASE A: Restore Defaults
            // 1. Clear the persistent storage (wipe non-volatile memory).
            s.clear()?;
            
            // 2. Clear the flag ONLY after the wipe succeeds.
            // This ensures transaction safety: if clear() fails, we retry on next boot.
            s.clear_restore_defaults_flag()?;
            
            info!("Storage cleared. OD initialized with Firmware/Protocol defaults.");
            // 3. Do NOT load. The OD remains at the state defined by steps 1 & 2.
        } else {
            // CASE B: Normal Boot
            info!("Loading parameters from persistent storage.");
            
            // 1. Load parameters from storage.
            let stored_params = s.load()?;
            
            // 2. Apply them to the OD (Overlay).
            let mut loaded_count = 0;
            for ((index, sub_index), value) in stored_params {
                // We use write_internal with check_access=false.
                // This allows restoring values even if the OD entry is technically ReadOnly 
                // to the network (e.g. configured static parameters).
                match od.write_internal(index, sub_index, value, false) {
                    Ok(_) => loaded_count += 1,
                    Err(e) => {
                        // This is not critical: it might be an orphaned parameter from 
                        // an old firmware version that no longer exists in the OD.
                        warn!("Failed to restore stored parameter {:#06X}/{}: {:?}", index, sub_index, e);
                    }
                }
            }
            info!("Restored {} parameters from storage.", loaded_count);
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

    struct MockStorage {
        saved_data: BTreeMap<(u16, u8), ObjectValue>,
        restore_requested: bool,
        // Tracking calls
        clear_called: bool,
        flag_cleared_called: bool,
    }
    impl MockStorage {
        fn new() -> Self {
            Self {
                saved_data: BTreeMap::new(),
                restore_requested: false,
                clear_called: false,
                flag_cleared_called: false,
            }
        }
    }
    impl ObjectDictionaryStorage for MockStorage {
        fn load(&mut self) -> Result<BTreeMap<(u16, u8), ObjectValue>, PowerlinkError> {
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
            self.flag_cleared_called = true;
            self.restore_requested = false;
            Ok(())
        }
    }

    #[test]
    fn test_init_normal_boot_loads_values() {
        let mut storage = MockStorage::new();
        // Storage contains a value different from default
        storage
            .saved_data
            .insert((0x6000, 0), ObjectValue::Unsigned32(999));

        let mut od = ObjectDictionary::new(Some(&mut storage));
        od.insert(
            0x6000,
            ObjectEntry {
                object: Object::Variable(ObjectValue::Unsigned32(0)), // Firmware default is 0
                name: "StorableVar",
                category: Category::Optional,
                access: Some(AccessType::ReadWriteStore),
                ..Default::default()
            },
        );

        init(&mut od).unwrap();

        // Should have loaded 999 from storage
        assert_eq!(od.read_u32(0x6000, 0).unwrap(), 999);
    }

    #[test]
    fn test_init_restore_defaults_clears_and_uses_defaults() {
        let mut storage = MockStorage::new();
        // Storage has data
        storage
            .saved_data
            .insert((0x6000, 0), ObjectValue::Unsigned32(999));
        // Flag is set
        storage.restore_requested = true;

        {
            let mut od = ObjectDictionary::new(Some(&mut storage));
            od.insert(
                0x6000,
                ObjectEntry {
                    object: Object::Variable(ObjectValue::Unsigned32(0)), // Firmware default is 0
                    name: "StorableVar",
                    category: Category::Optional,
                    access: Some(AccessType::ReadWriteStore),
                    default_value: Some(ObjectValue::Unsigned32(0)),
                    ..Default::default()
                },
            );

            init(&mut od).unwrap();

            // 1. OD should be 0 (Firmware Default), NOT 999
            assert_eq!(od.read_u32(0x6000, 0).unwrap(), 0);
        }
        
        // 2. Storage backend checks
        assert!(storage.clear_called, "Storage.clear() should have been called");
        assert!(storage.flag_cleared_called, "Storage.clear_restore_defaults_flag() should have been called");
        // Note: In a real mock, clear() clears the map, but our flag logic is boolean.
        // The important part is the method calls.
    }
}