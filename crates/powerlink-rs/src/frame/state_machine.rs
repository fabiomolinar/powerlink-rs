//+ NEW FILE
use crate::nmt::states::NMTState;

/// States for the Data Link Layer Cycle State Machine (DLL_CS) of a CN.
/// This machine tracks the expected sequence of frames within a single POWERLINK cycle.
/// (EPSG DS 301, Section 4.2.4.5.2)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DllState {
    /// The isochronous communication is not active.
    #[default]
    NonCyclic,
    /// Waiting for the Start of Cycle (SoC) frame.
    WaitSoc,
    /// Waiting for a Poll Request (PReq) frame.
    WaitPReq,
    /// Waiting for the Start of Asynchronous (SoA) frame.
    WaitSoA,
}

/// Events that drive the DLL_CS, corresponding to received frames or timeouts.
/// (EPSG DS 301, Section 4.2.4.5.3)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DllEvent {
    SoCReceived,
    PReqReceived,
    SoAReceived,
    SoCTimeout,
}

/// Manages the DLL cycle state for a Controlled Node.
pub struct DllStateMachine {
    state: DllState,
}

impl DllStateMachine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Processes an incoming event and transitions the state based on the current NMT state.
    /// The logic follows the state diagram in Figure 30 of the specification.
    pub fn process_event(&mut self, event: DllEvent, nmt_state: NMTState) {
        // The DLL_CS is active only in specific NMT states.
        match nmt_state {
            NMTState::PreOperational2 | NMTState::ReadyToOperate | NMTState::Operational | NMTState::Stopped => {
                let next_state = match (self.state, event) {
                    // A SoC can be received in any state and always resets the cycle to WaitPReq.
                    // This covers (DLL_CT1), (DLL_CT7), and (DLL_CT9).
                    (_, DllEvent::SoCReceived) => DllState::WaitPReq,

                    // (DLL_CT2) Happy path: PReq is received while waiting for it.
                    (DllState::WaitPReq, DllEvent::PReqReceived) => DllState::WaitSoA,

                    // (DLL_CT3) Happy path: SoA is received while waiting for it.
                    (DllState::WaitSoA, DllEvent::SoAReceived) => DllState::WaitSoc,
                    
                    // --- Handling of Lost Frames ---
                    // (DLL_CT3, part of DLL_CE_PREQ) Lost SoA and SoC: A PReq is received while waiting for the next SoC.
                    // The node processes the PReq and moves on.
                    (DllState::WaitSoc, DllEvent::PReqReceived) => DllState::WaitSoA,

                    // (DLL_CT8) Lost PReq: An SoA is received while waiting for a PReq.
                    // This is normal for multiplexed or stopped nodes.
                    (DllState::WaitPReq, DllEvent::SoAReceived) => DllState::WaitSoc,
                    
                    // A timeout for the SoC frame indicates a loss of communication, resetting the state.
                    (_, DllEvent::SoCTimeout) => DllState::NonCyclic,

                    // If an unexpected event occurs, remain in the current state.
                    // Error reporting would be triggered here.
                    (current, _) => current,
                };
                self.state = next_state;
            },
            _ => {
                // In all other NMT states, the Dll state machine is considered non-cyclic.
                self.state = DllState::NonCyclic;
            }
        }
    }

    /// Returns the current state of the DLL state machine.
    pub fn current_state(&self) -> DllState {
        self.state
    }
}

impl Default for DllStateMachine {
    fn default() -> Self {
        Self { state: DllState::NonCyclic }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dll_cn_happy_path_transitions() {
        let mut sm = DllStateMachine::new();
        let operational_state = NMTState::Operational;

        // Initial state
        assert_eq!(sm.current_state(), DllState::NonCyclic);

        // A SoC starts the cycle
        sm.process_event(DllEvent::SoCReceived, operational_state);
        assert_eq!(sm.current_state(), DllState::WaitPReq);

        // A PReq is received
        sm.process_event(DllEvent::PReqReceived, operational_state);
        assert_eq!(sm.current_state(), DllState::WaitSoA);
        
        // An SoA ends the isochronous phase
        sm.process_event(DllEvent::SoAReceived, operational_state);
        assert_eq!(sm.current_state(), DllState::WaitSoc);
        
        // A timeout resets the state machine
        sm.process_event(DllEvent::SoCTimeout, operational_state);
        assert_eq!(sm.current_state(), DllState::NonCyclic);
    }

    #[test]
    fn test_dll_cn_lost_frame_recovery() {
        let mut sm = DllStateMachine::new();
        let operational_state = NMTState::Operational;
        
        // Start a cycle
        sm.process_event(DllEvent::SoCReceived, operational_state);
        sm.process_event(DllEvent::PReqReceived, operational_state);
        sm.process_event(DllEvent::SoAReceived, operational_state);
        assert_eq!(sm.current_state(), DllState::WaitSoc);

        // SCENARIO: SoA from previous cycle and SoC from new cycle were lost.
        // The CN receives a PReq for the new cycle while still in WaitSoc.
        sm.process_event(DllEvent::PReqReceived, operational_state);
        
        // The state machine should recover and move to WaitSoA.
        assert_eq!(sm.current_state(), DllState::WaitSoA);
    }
}