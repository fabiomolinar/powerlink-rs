#![cfg_attr(not(feature = "std"), no_std)]

// If the 'alloc' feature is enabled, import the crate necessary for dynamic allocation (e.g., Vec<u8> in frames)
// This is necessary since frame payloads use Vec<u8>.
#[cfg(feature = "alloc")]
extern crate alloc;

// --- Foundation Modules ---
pub mod types;
pub mod hal;

// --- Data Link Layer (DLL) implementation (Phase 1 Focus) ---
pub mod frame;

// --- Higher Layers (Phase 2+ Focus, currently empty structures) ---
pub mod nmt;
pub mod od;
pub mod pdo;
pub mod sdo;
    
// Export core types and the Network Interface abstraction
pub use types::{NodeId, UNSIGNED8, UNSIGNED16, UNSIGNED32};
pub use hal::{NetworkInterface, PowerlinkError};