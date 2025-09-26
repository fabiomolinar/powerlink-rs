#![allow(non_camel_case_types)]

use crate::nmt::states::NMTState;

/// States for the Data Link Layer Cycle State Machine (DLL_CS) of a CN.
/// This machine tracks the expected sequence of frames within a single POWERLINK cycle.
/// (EPSG DS 301, Section 4.2.4.5.2)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DllCsState {
    /// The isochronous communication is not active.
    #[default]
    DLL_CS_NON_CYCLIC,
    /// Waiting for the Start of Cycle (SoC) frame.
    DLL_CS_WAIT_SOC,
    /// Waiting for a Poll Request (PReq) frame.
    DLL_CS_WAIT_PREQ,
    /// Waiting for the Start of Asynchronous (SoA) frame.
    DLL_CS_WAIT_SOA,
}

/// Events that drive the DLL_CS, corresponding to received frames or timeouts.
/// (EPSG DS 301, Section 4.2.4.5.3)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DllCsEvent {
    DLL_CE_SOC,
    DLL_CE_PREQ,
    DLL_CE_PRES,
    DLL_CE_SOA,
    DLL_CE_ASND,
    DLL_CE_SOC_TIMEOUT
}

/// Manages the DLL cycle state for a Controlled Node (CN).
pub struct DllCsStateMachine {
    state: DllCsState,
}

impl DllCsStateMachine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Processes an incoming event and transitions the state based on the current NMT state.
    /// The logic follows the state diagram in Figure 30 of the specification.
    pub fn process_event(&mut self, event: DllCsEvent, nmt_state: NMTState) {
        // The DLL_CS is active only in specific NMT states.
        match nmt_state {
            NMTState::NMT_CS_PRE_OPERATIONAL_2 | NMTState::NMT_CS_READY_TO_OPERATE | NMTState::NMT_CS_OPERATIONAL | NMTState::NMT_CS_STOPPED => {
                let next_state = match (self.state, event) {
                    // --- (DLL_CT02) ---
                    // Process the PReq frame and send a PRes frame
                    (DllCsState::DLL_CS_WAIT_PREQ, DllCsEvent::DLL_CE_PREQ) => DllCsState::DLL_CS_WAIT_SOA,
                    // --- (DLL_CT03) ---
                    // Process SoA, if allowed send an ASnd frame or a non POWERLINK frame
                    (DllCsState::DLL_CS_WAIT_SOA, DllCsEvent::DLL_CE_SOA) => DllCsState::DLL_CS_WAIT_SOC,
                    // Accept the PReq frame and send a PRes frame, report error DLL_CEV_LOSS_SOC and DLL_CEV_LOSS_SOA
                    (DllCsState::DLL_CS_WAIT_SOA, DllCsEvent::DLL_CE_PREQ) => DllCsState::DLL_CS_WAIT_SOC,
                    // Synchronise to the next SoC, report error DLL_CEV_LOSS_SOC and DLL_CEV_LOSS_SOA
                    (DllCsState::DLL_CS_WAIT_SOA, DllCsEvent::DLL_CE_SOC_TIMEOUT) => DllCsState::DLL_CS_WAIT_SOC,
                    // --- (DLL_CT04) ---
                    // Process frame
                    (DllCsState::DLL_CS_WAIT_SOC, DllCsEvent::DLL_CE_ASND) => DllCsState::DLL_CS_WAIT_SOC,
                    // Respond with PRes frame, report error DLL_CEV_LOSS_SOC
                    (DllCsState::DLL_CS_WAIT_SOC, DllCsEvent::DLL_CE_PREQ) => DllCsState::DLL_CS_WAIT_SOC,
                    // Report error DLL_CEV_LOSS_SOC
                    (DllCsState::DLL_CS_WAIT_SOC, DllCsEvent::DLL_CE_PRES) => DllCsState::DLL_CS_WAIT_SOC,
                    // Report error DLL_CEV_LOSS_SOC
                    (DllCsState::DLL_CS_WAIT_SOC, DllCsEvent::DLL_CE_SOA) => DllCsState::DLL_CS_WAIT_SOC,                    
                    // Report error DLL_CEV_LOSS_SOC
                    (DllCsState::DLL_CS_WAIT_SOC, DllCsEvent::DLL_CE_SOC_TIMEOUT) => DllCsState::DLL_CS_WAIT_SOC,
                    // --- (DLL_CT07) ---
                    // Process PRes frames (cross traffic)
                    (DllCsState::DLL_CS_WAIT_PREQ, DllCsEvent::DLL_CE_PRES) => DllCsState::DLL_CS_WAIT_PREQ,
                    // Report error DLL_CEV_LOSS_SOA
                    (DllCsState::DLL_CS_WAIT_PREQ, DllCsEvent::DLL_CE_ASND) => DllCsState::DLL_CS_WAIT_PREQ, 
                    // Synchronise to the cycle begin, report error DLL_CEV_LOSS_SOA
                    (DllCsState::DLL_CS_WAIT_PREQ, DllCsEvent::DLL_CE_SOC) => DllCsState::DLL_CS_WAIT_PREQ, 
                    // --- (DLL_CT08) ---
                    // Process SoA, if invited, transmit a legal Ethernet frame
                    (DllCsState::DLL_CS_WAIT_PREQ, DllCsEvent::DLL_CE_SOA) => DllCsState::DLL_CS_WAIT_SOC,
                    //  Synchronise on the next SoC, report error DLL_CEV_LOSS_SOC and DLL_CEV_LOSS_SOA
                    (DllCsState::DLL_CS_WAIT_PREQ, DllCsEvent::DLL_CE_SOC_TIMEOUT) => DllCsState::DLL_CS_WAIT_SOC, 
                    // --- (DLL_CT09) ---
                    // Synchronise on the SoC, report error DLL_CEV_LOSS_SOA
                    (DllCsState::DLL_CS_WAIT_SOA, DllCsEvent::DLL_CE_SOC) => DllCsState::DLL_CS_WAIT_PREQ,
                    // --- (DLL_CT10) ---
                    // Process PRes frames (cross traffic)
                    (DllCsState::DLL_CS_WAIT_SOA, DllCsEvent::DLL_CE_PRES) => DllCsState::DLL_CS_WAIT_SOA,
                    // Report error DLL_CEV_LOSS_SOA 
                    (DllCsState::DLL_CS_WAIT_SOA, DllCsEvent::DLL_CE_ASND) => DllCsState::DLL_CS_WAIT_SOA,                 
                    // --- (DLL_CT01) ---
                    // A SoC can be received in any state and always resets the cycle to WaitPReq.
                    // Synchronise the start of cycle and generate a SoC trigger to the application
                    (_, DllCsEvent::DLL_CE_SOC) => DllCsState::DLL_CS_WAIT_PREQ,

                    // If an unexpected event occurs, remain in the current state.
                    // Error reporting would be triggered here.
                    (current, _) => current,
                };
                self.state = next_state;
            },
            _ => {
                // In all other NMT states, the Dll state machine is considered non-cyclic.
                self.state = DllCsState::DLL_CS_NON_CYCLIC;
            }
        }
    }

    /// Returns the current state of the DLL state machine.
    pub fn current_state(&self) -> DllCsState {
        self.state
    }
}

impl Default for DllCsStateMachine {
    fn default() -> Self {
        Self { state: DllCsState::DLL_CS_NON_CYCLIC }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dll_cn_happy_path_transitions() {
        let mut sm = DllCsStateMachine::new();
        let operational_state = NMTState::NMT_CS_OPERATIONAL;

        // Initial state
        assert_eq!(sm.current_state(), DllCsState::DLL_CS_NON_CYCLIC);

        // A SoC starts the cycle
        sm.process_event(DllCsEvent::DLL_CE_SOC, operational_state);
        assert_eq!(sm.current_state(), DllCsState::DLL_CS_WAIT_PREQ);

        // A PReq is received
        sm.process_event(DllCsEvent::DLL_CE_PREQ, operational_state);
        assert_eq!(sm.current_state(), DllCsState::DLL_CS_WAIT_SOA);
        
        // An SoA ends the isochronous phase
        sm.process_event(DllCsEvent::DLL_CE_PRES, operational_state);
        assert_eq!(sm.current_state(), DllCsState::DLL_CS_WAIT_SOC);
        
        // A timeout resets the state machine
        sm.process_event(DllCsEvent::DLL_CE_SOA, operational_state);
        assert_eq!(sm.current_state(), DllCsState::DLL_CS_NON_CYCLIC);
    }

    #[test]
    fn test_dll_cn_lost_frame_recovery() {
        let mut sm = DllCsStateMachine::new();
        let operational_state = NMTState::NMT_CS_OPERATIONAL;
        
        // Start a cycle
        sm.process_event(DllCsEvent::DLL_CE_SOC, operational_state);
        sm.process_event(DllCsEvent::DLL_CE_PREQ, operational_state);
        sm.process_event(DllCsEvent::DLL_CE_PRES, operational_state);
        assert_eq!(sm.current_state(), DllCsState::DLL_CS_WAIT_SOC);

        // SCENARIO: SoA from previous cycle and SoC from new cycle were lost.
        // The CN receives a PReq for the new cycle while still in WaitSoc.
        sm.process_event(DllCsEvent::DLL_CE_PREQ, operational_state);
        
        // The state machine should recover and move to WaitSoA.
        assert_eq!(sm.current_state(), DllCsState::DLL_CS_WAIT_SOA);
    }
}