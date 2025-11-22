use alloc::string::String;
use alloc::format;

/// Trait for structs that provide metadata for logging
pub trait LogMetadata {
    fn meta(&self) -> String;
}

/// Example context struct
pub struct LogContext {
    pub system: &'static str,
    pub component: &'static str,
    pub id: u32,
}

impl LogMetadata for LogContext {
    fn meta(&self) -> String {
        format!(
            "system={}, component={}, id={}",
            self.system, self.component, self.id
        )
    }
}

// =============================================
// Logging Macros (namespaced under crate::log)
// =============================================

// ===== my_info! =====
macro_rules! my_info {
    ($ctx:expr, $fmt:literal $(, $($arg:tt)+)?) => {{
        let meta = $crate::log::LogMetadata::meta(&$ctx);
        log::info!(concat!("[{}] ", $fmt), meta $(, $($arg)+)?);
    }};
    ($fmt:literal $(, $($arg:tt)+)?) => {{
        log::info!($fmt $(, $($arg)+)?);
    }};
}

// ===== my_warn! =====
macro_rules! my_warn {
    ($ctx:expr, $fmt:literal $(, $($arg:tt)+)?) => {{
        let meta = $crate::log::LogMetadata::meta(&$ctx);
        log::warn!(concat!("[{}] ", $fmt), meta $(, $($arg)+)?);
    }};
    ($fmt:literal $(, $($arg:tt)+)?) => {{
        log::warn!($fmt $(, $($arg)+)?);
    }};
}

// ===== my_error! =====
macro_rules! my_error {
    ($ctx:expr, $fmt:literal $(, $($arg:tt)+)?) => {{
        let meta = $crate::log::LogMetadata::meta(&$ctx);
        log::error!(concat!("[{}] ", $fmt), meta $(, $($arg)+)?);
    }};
    ($fmt:literal $(, $($arg:tt)+)?) => {{
        log::error!($fmt $(, $($arg)+)?);
    }};
}

// ===== my_debug! =====
macro_rules! my_debug {
    ($ctx:expr, $fmt:literal $(, $($arg:tt)+)?) => {{
        let meta = $crate::log::LogMetadata::meta(&$ctx);
        log::debug!(concat!("[{}] ", $fmt), meta $(, $($arg)+)?);
    }};
    ($fmt:literal $(, $($arg:tt)+)?) => {{
        log::debug!($fmt $(, $($arg)+)?);
    }};
}

// ===== my_trace! =====
macro_rules! my_trace {
    ($ctx:expr, $fmt:literal $(, $($arg:tt)+)?) => {{
        let meta = $crate::log::LogMetadata::meta(&$ctx);
        log::trace!(concat!("[{}] ", $fmt), meta $(, $($arg)+)?);
    }};
    ($fmt:literal $(, $($arg:tt)+)?) => {{
        log::trace!($fmt $(, $($arg)+)?);
    }};
}

// Re-export macros for use in other files
pub(crate) use my_info;
pub(crate) use my_warn;
pub(crate) use my_error;
pub(crate) use my_debug;
pub(crate) use my_trace;
