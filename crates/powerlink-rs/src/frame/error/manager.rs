use super::traits::{ErrorCounters, ErrorHandler};
use super::types::{DllError, NmtAction};

/// The central manager, generic over the counter set and the handler.
pub struct DllErrorManager<C, H>
where
    C: ErrorCounters,
    H: ErrorHandler,
{
    pub counters: C,
    pub handler: H,
}

impl<C, H> DllErrorManager<C, H>
where
    C: ErrorCounters,
    H: ErrorHandler,
{
    pub fn new(counters: C, handler: H) -> Self {
        Self { counters, handler }
    }

    pub fn handle_error(&mut self, error: DllError) -> (NmtAction, bool) {
        self.counters.handle_error(error, &mut self.handler)
    }

    pub fn on_cycle_complete(&mut self) -> bool {
        self.counters.on_cycle_complete()
    }
}
