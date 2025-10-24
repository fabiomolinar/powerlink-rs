use super::events::NmtEvent;
use super::states::NmtState;
use crate::frame::DllError;
use crate::od::{ObjectDictionary, ObjectValue};
use alloc::vec::Vec;
use log::info;

/// A trait defining the common behavior for all NMT state machines (MN and CN).
pub trait NmtStateMachine {
    /// Returns the current NMT state.
    fn current_state(&self) -> NmtState;

    /// Sets the internal NMT state. Required for default trait methods.
    fn set_state(&mut self, new_state: NmtState);

    /// Processes an external event and transitions the NMT state accordingly.
    fn process_event(
        &mut self,
        event: NmtEvent,
        od: &mut ObjectDictionary,
    ) -> Option<Vec<DllError>>;

    /// Writes the current NMT state to the Object Dictionary (Index 0x1F8C).
    /// This is a provided method to reduce code duplication.
    fn update_od_state(&self, od: &mut ObjectDictionary) {
        // This write is internal and should not fail. `unwrap` is acceptable here.
        od.write_internal(
            0x1F8C,
            0,
            ObjectValue::Unsigned8(self.current_state() as u8),
            false,
        )
        .unwrap();
    }

    /// Resets the state machine to a specific reset state. This is a default implementation.
    fn reset(&mut self, event: NmtEvent) {
        let new_state = match event {
            NmtEvent::Reset | NmtEvent::SwReset => NmtState::NmtGsInitialising,
            NmtEvent::ResetNode => NmtState::NmtGsResetApplication,
            NmtEvent::ResetCommunication => NmtState::NmtGsResetCommunication,
            NmtEvent::ResetConfiguration => NmtState::NmtGsResetConfiguration,
            _ => return, // Not a reset event for this handler
        };
        info!(
            "[NMT] State reset from {:?} to {:?} due to event {:?}",
            self.current_state(),
            new_state,
            event
        );
        self.set_state(new_state);
    }

    /// Handles automatic, internal state transitions that don't require an external event.
    /// This is the common boot sequence for both MN and CN.
    fn run_internal_initialisation(&mut self, od: &mut ObjectDictionary) {
        // This sequence is only valid if starting from Initialising.
        if self.current_state() != NmtState::NmtGsInitialising {
            return;
        }
        info!("Starting internal NMT initialisation sequence.");

        let sequence = [
            NmtState::NmtGsResetApplication,
            NmtState::NmtGsResetCommunication,
            NmtState::NmtGsResetConfiguration,
            NmtState::NmtNotActive,
        ];

        for &next_state in &sequence {
            info!(
                "[NMT] Internal transition from {:?} to {:?}",
                self.current_state(),
                next_state
            );
            self.set_state(next_state);
            self.update_od_state(od);
        }
        info!("Internal NMT initialisation sequence complete.");
    }
}
