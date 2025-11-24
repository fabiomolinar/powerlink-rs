// src/log.rs
use alloc::string::String;
use alloc::format;

/// A trait for objects that can provide a contextual prefix for log messages.
/// 
/// Implementing this for Nodes allows the `pl_*` macros to automatically 
/// prepend node identification (e.g., "MN - Node 240: ") to log lines.
pub trait Loggable {
    fn log_prefix(&self) -> String;
}

/// Logs an error message.
///
/// Usage:
/// - `pl_error!("Message")`
/// - `pl_error!(node_ref, "Message with context")`
macro_rules! pl_error {
    ($target:expr, $($arg:tt)+) => {
        if let Some(prefix) = $crate::log::try_get_prefix(&$target) {
             ::log::error!("{} {}", prefix, format_args!($($arg)+));
        } else {
             ::log::error!($($arg)+);
        }
    };
    ($($arg:tt)+) => {
        ::log::error!($($arg)+);
    }
}

/// Logs a warning message.
macro_rules! pl_warn {
    ($target:expr, $($arg:tt)+) => {
        if let Some(prefix) = $crate::log::try_get_prefix(&$target) {
             ::log::warn!("{} {}", prefix, format_args!($($arg)+));
        } else {
             ::log::warn!($($arg)+);
        }
    };
    ($($arg:tt)+) => {
        ::log::warn!($($arg)+);
    }
}

/// Logs an info message.
macro_rules! pl_info {
    ($target:expr, $($arg:tt)+) => {
        if let Some(prefix) = $crate::log::try_get_prefix(&$target) {
             ::log::info!("{} {}", prefix, format_args!($($arg)+));
        } else {
             ::log::info!($($arg)+);
        }
    };
    ($($arg:tt)+) => {
        ::log::info!($($arg)+);
    }
}

/// Logs a debug message.
macro_rules! pl_debug {
    ($target:expr, $($arg:tt)+) => {
        if let Some(prefix) = $crate::log::try_get_prefix(&$target) {
             ::log::debug!("{} {}", prefix, format_args!($($arg)+));
        } else {
             ::log::debug!($($arg)+);
        }
    };
    ($($arg:tt)+) => {
        ::log::debug!($($arg)+);
    }
}

/// Logs a trace message.
macro_rules! pl_trace {
    ($target:expr, $($arg:tt)+) => {
        if let Some(prefix) = $crate::log::try_get_prefix(&$target) {
             ::log::trace!("{} {}", prefix, format_args!($($arg)+));
        } else {
             ::log::trace!($($arg)+);
        }
    };
    ($($arg:tt)+) => {
        ::log::trace!($($arg)+);
    }
}

// Export the macros

pub(crate) use pl_error;
pub(crate) use pl_warn;
pub(crate) use pl_info;
pub(crate) use pl_debug;
pub(crate) use pl_trace;

// Helper to check if a type implements Loggable safely within the macro.
// This uses a specialization trick or simple trait bounds if possible.
// Since we can't easily do conditional compilation inside the macro based on type,
// we use a helper function that accepts &T.
// However, to support `pl_info!("msg")` where the first arg is a string literal,
// the macro matcher `$target:expr` handles the split.
// The issue is distinguishing `pl_info!(node, "msg")` vs `pl_info!("format", arg)`.
//
// The macros above attempt to match `$target:expr` as the node. 
// BUT, string literals are also expressions.
// `pl_info!("msg")` matches `$target:expr` with "msg".
// `try_get_prefix` will return None for &str, so it falls back to standard logging, 
// but the format string handling gets messy.
//
// REVISED STRATEGY for Macros:
// We rely on the comma separator to distinguish. 
// `pl_info!(node, "fmt", args...)` has a comma after the first expr.
// `pl_info!("fmt", args...)` also has a comma.
// 
// To strictly differentiate, we define a helper trait `MaybeLoggable` implemented for `T: Loggable` 
// and `str` (returning None).

pub fn try_get_prefix<T: ?Sized + LoggableWithDefault>(t: &T) -> Option<String> {
    t.as_loggable()
}

pub trait LoggableWithDefault {
    fn as_loggable(&self) -> Option<String>;
}

impl<T: Loggable> LoggableWithDefault for T {
    fn as_loggable(&self) -> Option<String> {
        Some(self.log_prefix())
    }
}

// Fallback for string literals and other types passed as first arg in standard logging
impl LoggableWithDefault for str {
    fn as_loggable(&self) -> Option<String> {
        None
    }
}
impl LoggableWithDefault for &str {
    fn as_loggable(&self) -> Option<String> {
        None
    }
}