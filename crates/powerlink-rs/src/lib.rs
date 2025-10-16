#![cfg_attr(not(feature = "std"), no_std)]


// 'alloc' is used for dynamic allocation (e.g., Vec<u8> in frames)
extern crate alloc;

// --- Foundation Modules ---
pub mod types;
pub mod hal;
pub mod common;

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
pub use types::NodeId;
pub use hal::{NetworkInterface, PowerlinkError, ObjectDictionaryStorage};
pub use common::{NetTime, RelativeTime};
pub use frame::error::{ErrorHandler, DllErrorManager, NoOpErrorHandler};
pub use frame::codec::{Codec, deserialize_frame};
pub use node::{Node, NodeAction};
pub use node::cn::ControlledNode;
