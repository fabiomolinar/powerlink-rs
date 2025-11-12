//! Centralizes all Data Link Layer error handling logic, including traits,
//! types, counters, and the main error manager.

pub mod counters;
pub mod manager;
pub mod status_response;
pub mod traits;
pub mod types;

pub use counters::{CnErrorCounters, MnErrorCounters, ThresholdCounter};
pub use manager::DllErrorManager;
pub use status_response::{EntryType, ErrorEntry, ErrorEntryMode};
pub use traits::{ErrorCounters, ErrorHandler, LoggingErrorHandler};
pub use types::{DllError, NmtAction};

// Re-export common handler implementations for convenience.
pub use traits::NoOpErrorHandler;
#[cfg(feature = "std")]
pub use traits::StdoutErrorHandler;