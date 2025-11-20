// crates/powerlink-rs/src/nmt/state_machine.rs
use super::events::NmtEvent;
use super::states::NmtState;
use crate::NodeId;
use crate::frame::DllError;
use crate::od::{ObjectDictionary, ObjectValue};
use alloc::vec::Vec;
use log::{error, info};

/// A trait defining the common behavior for all NMT state machines (MN and CN).
pub trait NmtStateMachine {
    /// Returns the Node ID associated with this NMT state machine.
    fn node_id(&self) -> NodeId;

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
        // Attempt to write the NMT state to the OD.
        // While this write is internal and unlikely to fail, proper error handling
        // prevents potential panics in embedded environments.
        if let Err(e) = od.write_internal(
            0x1F8C,
            0,
            ObjectValue::Unsigned8(self.current_state() as u8),
            false,
        ) {
            error!("[NMT] Failed to update NMT state in OD (0x1F8C): {:?}", e);
        }
    }

    /// Resets the state machine to a specific reset state and performs OD cleanup.
    /// (Reference: EPSG DS 301, 7.1.2.1.1.1 Sub-states)
    fn reset(&mut self, event: NmtEvent, od: &mut ObjectDictionary) {
        let initial_reset_state = match event {
            NmtEvent::Reset | NmtEvent::SwReset => NmtState::NmtGsInitialising,
            NmtEvent::ResetNode => NmtState::NmtGsResetApplication,
            NmtEvent::ResetCommunication => NmtState::NmtGsResetCommunication,
            NmtEvent::ResetConfiguration => NmtState::NmtGsResetConfiguration,
            _ => return, // Not a reset event for this handler
        };

        info!(
            "[NMT] Reset Sequence initiated from {:?} (Target: {:?}, Event: {:?})",
            self.current_state(),
            initial_reset_state,
            event
        );

        // Execute the cascading reset sequence.
        // Depending on the entry point, we fall through to subsequent states.
        
        // 1. NMT_GS_INITIALISING (PowerOn / SwReset)
        if initial_reset_state == NmtState::NmtGsInitialising {
            self.set_state(NmtState::NmtGsInitialising);
            self.update_od_state(od);
            // Basic initialization actions (if any)
        }

        // 2. NMT_GS_RESET_APPLICATION (ResetNode)
        // Fallthrough from Initialising OR start here
        if initial_reset_state == NmtState::NmtGsInitialising 
           || initial_reset_state == NmtState::NmtGsResetApplication 
        {
            self.set_state(NmtState::NmtGsResetApplication);
            self.update_od_state(od);
            
            info!("[NMT] NMT_GS_RESET_APPLICATION: Resetting App Parameters (0x6000-0x9FFF) and Manuf. (0x2000-0x5FFF)");
            // Reset Manufacturer Specific Profile Area (0x2000 - 0x5FFF)
            od.restore_power_on_values(0x2000, 0x5FFF);
            // Reset Standardised Device Profile Area (0x6000 - 0x9FFF)
            od.restore_power_on_values(0x6000, 0x9FFF);
        }

        // 3. NMT_GS_RESET_COMMUNICATION (ResetCommunication)
        // Fallthrough or start here
        if initial_reset_state == NmtState::NmtGsInitialising 
           || initial_reset_state == NmtState::NmtGsResetApplication
           || initial_reset_state == NmtState::NmtGsResetCommunication
        {
            self.set_state(NmtState::NmtGsResetCommunication);
            self.update_od_state(od);
            
            info!("[NMT] NMT_GS_RESET_COMMUNICATION: Resetting Comm Parameters (0x1000-0x1FFF)");
            // Reset Communication Profile Area (0x1000 - 0x1FFF), excluding Error History
            od.restore_power_on_values(0x1000, 0x1FFF);
        }

        // 4. NMT_GS_RESET_CONFIGURATION (ResetConfiguration)
        // Fallthrough or start here
        if initial_reset_state == NmtState::NmtGsInitialising 
           || initial_reset_state == NmtState::NmtGsResetApplication
           || initial_reset_state == NmtState::NmtGsResetCommunication
           || initial_reset_state == NmtState::NmtGsResetConfiguration
        {
            self.set_state(NmtState::NmtGsResetConfiguration);
            self.update_od_state(od);
            
            // Configuration actions could happen here (e.g., reloading NodeID from HW switches)
        }

        // Final Transition: Enter NMT_MS_NOT_ACTIVE or NMT_CS_NOT_ACTIVE
        // The specific MN or CN state machine will determine the final state (MT1 or CT1)
        // usually based on Node ID.
        // Since this trait is shared, we can default to NotActive.
        self.set_state(NmtState::NmtNotActive);
        self.update_od_state(od);
        info!("[NMT] Reset Sequence complete. Entered NmtNotActive.");
    }

    /// Handles automatic, internal state transitions that don't require an external event.
    /// This is the common boot sequence for both MN and CN.
    fn run_internal_initialisation(&mut self, od: &mut ObjectDictionary) {
        // This sequence is only valid if starting from Initialising.
        if self.current_state() != NmtState::NmtGsInitialising {
            return;
        }
        // Reuse the reset logic for the initial boot sequence
        self.reset(NmtEvent::SwReset, od);
    }
}