#![allow(non_camel_case_types)]

use crate::nmt::states::NMTState;

/// States for the Data Link Layer Cycle State Machine (DLL_CS) of a CN.
/// This machine tracks the expected sequence of frames within a single POWERLINK cycle.
/// (EPSG DS 301, Section 4.2.4.5.2)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DllMsState {
    /// The isochronous communication is not active.
    #[default]
    DLL_MS_NON_CYCLIC,
    /// Remains in this state until the next cycle begins with a DLL_ME_SOC_TRIG.
    DLL_MS_WAIT_SOC_TRIG,
    /// Waiting for a Poll Response (PRes) frame.
    DLL_MS_WAIT_PRES,
    /// Waits in this state until the asynchronous phase ends with the event DLL_ME_SOC_TRIG.
    DLL_MS_WAIT_ASND,
    // Wait in this state until the timeout of the async phase elapsed or any Ethernet frame was received. 
    DLL_MS_WAIT_SOA
}

/// Events that drive the DLL_CS, corresponding to received frames or timeouts.
/// (EPSG DS 301, Section 4.2.4.5.3)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DllMsEvent {
    // PRes frame was received. 
    DLL_ME_PRES,
    // PRes frame was not (or not completely) received within a preconfigured time. 
    DLL_ME_PRES_TIMEOUT,
    // ASnd frame or an non POWERLINK frame was received. 
    DLL_ME_ASND,
    // ASnd frame was not (or not completely) received within a preconfigured time.
    DLL_ME_ASND_TIMEOUT,
    // This event triggers emission of the SoC frame and starts a new POWERLINK cycle.
    DLL_ME_SOC_TRIG,
    // This event means that a new reduced POWERLINK cycle shall start.
    DLL_ME_SOA_TRIG
}

/// Manages the DLL cycle state for a Controlled Node (CN).
pub struct DllMsStateMachine {
    state: DllMsState,
}

impl DllMsStateMachine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Processes an incoming event and transitions the state based on the current NMT state.
    /// The logic follows the state diagram in Figure 30 of the specification.
    pub fn process_event(
        &mut self, event: DllMsEvent, nmt_state: NMTState, response_expected: bool, 
        async_in: bool, async_out: bool, isochr: bool
    ) {
        // The DLL_CS is active only in specific NMT states.
        match nmt_state {
            NMTState::NMT_MS_PRE_OPERATIONAL_1 => {
                let next_state = match (self.state, event) {
                    (DllMsState::DLL_MS_WAIT_SOA, DllMsEvent::DLL_ME_SOA_TRIG) => {
                        match (async_in, response_expected, async_out) {
                            // --- (DLL_MT10) ---
                            // Send SoA with Invite
                            (false, false, _) => DllMsState::DLL_MS_WAIT_SOA,
                            // Send SoA with Invite to MN and send ASnd or non POWERLINK frame
                            (true, _, false) => DllMsState::DLL_MS_WAIT_SOA,
                            // Send SoA
                            (true, _, true) => DllMsState::DLL_MS_WAIT_ASND,
                            // --- (DLL_MT11) ---
                            // Send SoA with Invite
                            (false, true, _) => DllMsState::DLL_MS_WAIT_ASND,
                        }
                    },                    
                    (DllMsState::DLL_MS_WAIT_ASND, DllMsEvent::DLL_ME_ASND_TIMEOUT | DllMsEvent::DLL_ME_SOA_TRIG) => {
                        match (async_in, response_expected) {
                            // --- (DLL_MT12) ---
                            (true, _) => DllMsState::DLL_MS_WAIT_ASND,
                            (false, false) => DllMsState::DLL_MS_WAIT_SOA,
                            // --- (DLL_MT13) ---
                            (false, true) => DllMsState::DLL_MS_WAIT_SOA,
                        }
                    },
                    // If an unexpected event occurs, remain in the current state.
                    // Error reporting would be triggered here.
                    (current, _) => current,                    
                };
                self.state = next_state;
            },
            NMTState::NMT_MS_OPERATIONAL | NMTState::NMT_MS_READY_TO_OPERATE | NMTState::NMT_MS_PRE_OPERATIONAL_2 => {
                // TODO

            },
            _ => {
                // In all other NMT states, the Dll state machine is considered non-cyclic.
                self.state = DllMsState::DLL_MS_NON_CYCLIC;
            }
        }
    }

    /// Returns the current state of the DLL state machine.
    pub fn current_state(&self) -> DllMsState {
        self.state
    }
}

impl Default for DllMsStateMachine {
    fn default() -> Self {
        Self { state: DllMsState::DLL_MS_NON_CYCLIC }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dll_cn_happy_path_transitions() {
        let mut sm = DllMsStateMachine::new();
        let operational_state = NMTState::NMT_CS_OPERATIONAL;

        // Initial state
        assert_eq!(sm.current_state(), DllMsState::DLL_CS_NON_CYCLIC);

        // A SoC starts the cycle
        sm.process_event(DllMsEvent::DLL_CE_SOC, operational_state);
        assert_eq!(sm.current_state(), DllMsState::DLL_CS_WAIT_PREQ);

        // A PReq is received
        sm.process_event(DllMsEvent::DLL_CE_PREQ, operational_state);
        assert_eq!(sm.current_state(), DllMsState::DLL_CS_WAIT_SOA);
        
        // An SoA ends the isochronous phase
        sm.process_event(DllMsEvent::DLL_CE_PRES, operational_state);
        assert_eq!(sm.current_state(), DllMsState::DLL_CS_WAIT_SOC);
        
        // A timeout resets the state machine
        sm.process_event(DllMsEvent::DLL_CE_SOA, operational_state);
        assert_eq!(sm.current_state(), DllMsState::DLL_CS_NON_CYCLIC);
    }

    #[test]
    fn test_dll_cn_lost_frame_recovery() {
        let mut sm = DllMsStateMachine::new();
        let operational_state = NMTState::NMT_CS_OPERATIONAL;
        
        // Start a cycle
        sm.process_event(DllMsEvent::DLL_CE_SOC, operational_state);
        sm.process_event(DllMsEvent::DLL_CE_PREQ, operational_state);
        sm.process_event(DllMsEvent::DLL_CE_PRES, operational_state);
        assert_eq!(sm.current_state(), DllMsState::DLL_CS_WAIT_SOC);

        // SCENARIO: SoA from previous cycle and SoC from new cycle were lost.
        // The CN receives a PReq for the new cycle while still in WaitSoc.
        sm.process_event(DllMsEvent::DLL_CE_PREQ, operational_state);
        
        // The state machine should recover and move to WaitSoA.
        assert_eq!(sm.current_state(), DllMsState::DLL_CS_WAIT_SOA);
    }
}