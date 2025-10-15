// In crates/powerlink-rs/src/od/storage.rs
use crate::od::ObjectValue;
use alloc::collections::BTreeMap;

/// A trait for abstracting the non-volatile storage of OD parameters.
pub trait ObjectDictionaryStorage {
    /// Loads storable parameters from non-volatile memory.
    /// Returns a map of (Index, SubIndex) -> Value.
    fn load(&mut self) -> Result<BTreeMap<(u16, u8), ObjectValue>, &'static str>;

    /// Saves the given storable parameters to non-volatile memory.
    fn save(&mut self, parameters: &BTreeMap<(u16, u8), ObjectValue>) -> Result<(), &'static str>;
    
    /// Clears all stored parameters, forcing a load of defaults on next boot.
    fn clear(&mut self) -> Result<(), &'static str>;
}