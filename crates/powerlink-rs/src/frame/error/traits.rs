use super::types::{DllError, NmtAction};

/// A trait that defines how DLL errors are reported or logged.
pub trait ErrorHandler {
    fn log_error(&mut self, error: &DllError);
}

/// A `no_std` compatible error handler that does nothing.
pub struct NoOpErrorHandler;
impl ErrorHandler for NoOpErrorHandler {
    fn log_error(&mut self, _error: &DllError) {}
}

/// An example `std`-based error handler that prints errors to the console.
#[cfg(feature = "std")]
pub struct StdoutErrorHandler;
#[cfg(feature = "std")]
impl ErrorHandler for StdoutErrorHandler {
    fn log_error(&mut self, error: &DllError) {
        println!("[POWERLINK DLL ERROR]: {:?}", error);
    }
}

/// Defines the essential behaviors for a set of DLL error counters.
pub trait ErrorCounters: Sized {
    /// Called once per POWERLINK cycle to decrement all threshold counters.
    fn on_cycle_complete(&mut self);

    /// Processes a given error, updates the appropriate counter, and returns an NMT action if a threshold is met.
    fn handle_error<H: ErrorHandler>(&mut self, error: DllError, handler: &mut H) -> NmtAction;
}