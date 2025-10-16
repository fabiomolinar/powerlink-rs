use crate::frame::DllError;
use crate::od::{ObjectDictionary, ObjectValue};
use super::states::{NmtEvent, NmtState};
use alloc::vec::Vec;

/// A trait defining the common behavior for all NMT state machines (MN and CN).
pub trait NmtStateMachine {
    /// Returns the current NMT state.
    fn current_state(&self) -> NmtState;

    /// Processes an external event and transitions the NMT state accordingly.
    fn process_event(&mut self, event: NmtEvent, od: &mut ObjectDictionary) -> Option<Vec<DllError>>;

    /// Writes the current NMT state to the Object Dictionary (Index 0x1F8C).
    /// This is a provided method to reduce code duplication.
    fn update_od_state(&self, od: &mut ObjectDictionary) {
        // This write is internal and should not fail. `unwrap` is acceptable here.
        od.write_internal(0x1F8C, 0, ObjectValue::Unsigned8(self.current_state() as u8), false).unwrap();
    }

    /// Resets the state machine to a specific reset state.
    fn reset(&mut self, event: NmtEvent);

    /// Handles automatic, internal state transitions that don't require an external event.
    /// This is primarily for the node's boot sequence.
    fn run_internal_initialisation(&mut self, od: &mut ObjectDictionary);
}
