// In frame/error/mod.rs

//! Centralizes all Data Link Layer error handling logic, including traits,
//! types, counters, and the main error manager.

pub mod traits;
pub mod types;
pub mod counters;
pub mod manager;

pub use traits::{ErrorHandler, ErrorCounters};
pub use types::{DllError, NmtAction};
pub use counters::{CnErrorCounters, MnErrorCounters, ThresholdCounter};
pub use manager::DllErrorManager;

// Re-export common handler implementations for convenience.
#[cfg(feature = "std")]
pub use traits::StdoutErrorHandler;
pub use traits::NoOpErrorHandler;