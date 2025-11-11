#![cfg_attr(not(feature = "std"), no_std)]

// 'alloc' is used for dynamic allocation (e.g., Vec<u8> in frames)
extern crate alloc;

// --- Foundation Modules ---
pub mod common;
pub mod hal;
pub mod types;

// --- Node Abstraction ---
pub mod node;

// --- Data Link Layer (DLL) ---
pub mod frame;

// --- Higher Layers ---
pub mod nmt;
pub mod od;
pub mod pdo;
pub mod sdo;

// --- Top-level Exports ---
pub use common::{NetTime, RelativeTime};
pub use frame::codec::{Codec, deserialize_frame};
pub use frame::error::{DllErrorManager, ErrorHandler, LoggingErrorHandler, NoOpErrorHandler};
pub use hal::{NetworkInterface, ObjectDictionaryStorage, PowerlinkError};
pub use node::cn::ControlledNode;
pub use node::{Node, NodeAction};
pub use pdo::{PdoError, PdoMappingEntry}; // Export PdoError
pub use types::NodeId;
