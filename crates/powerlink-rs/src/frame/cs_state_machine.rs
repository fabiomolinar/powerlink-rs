use crate::nmt::states::NmtState;
use crate::frame::error::DllError;
use alloc::vec::Vec;

/// States for the Data Link Layer Cycle State Machine (DLL_CS) of a CN.
///
/// This machine tracks the expected sequence of frames within a single POWERLINK cycle.
/// (Reference: EPSG DS 301, Section 4.2.4.5.2)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DllCsState {
    /// The isochronous communication is not active. Corresponds to `DLL_CS_NON_CYCLIC`.
    #[default]
    NonCyclic,
    /// Waiting for the Start of Cycle (SoC) frame. Corresponds to `DLL_CS_WAIT_SOC`.
    WaitSoc,
    /// Waiting for a Poll Request (PReq) frame. Corresponds to `DLL_CS_WAIT_PREQ`.
    WaitPreq,
    /// Waiting for the Start of Asynchronous (SoA) frame. Corresponds to `DLL_CS_WAIT_SOA`.
    WaitSoa,
}

/// Events that drive the DLL_CS, corresponding to received frames or timeouts.
/// (Reference: EPSG DS 301, Section 4.2.4.5.3)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DllCsEvent {
    /// Corresponds to `DLL_CE_SOC`.
    Soc,
    /// Corresponds to `DLL_CE_PREQ`.
    Preq,
    /// Corresponds to `DLL_CE_PRES`.
    Pres,
    /// Corresponds to `DLL_CE_SOA`.
    Soa,
    /// Corresponds to `DLL_CE_ASND`.
    Asnd,
    /// Corresponds to `DLL_CE_SOC_TIMEOUT`.
    SocTimeout,
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
    pub fn process_event(&mut self, event: DllCsEvent, nmt_state: NmtState) -> Option<Vec<DllError>>{
        let mut errors: Vec<DllError> = Vec::new();
        // The DLL_CS is active only in specific NMT states.
        match nmt_state {
            NmtState::NmtCsPreOperational2 | NmtState::NmtCsReadyToOperate | NmtState::NmtCsOperational | NmtState::NmtCsStopped => {
                let next_state = match (self.state, event) {
                    // --- (DLL_CT02) ---
                    // Process the PReq frame and send a PRes frame
                    (DllCsState::WaitPreq, DllCsEvent::Preq) => DllCsState::WaitSoa,
                    // --- (DLL_CT03) ---
                    // Process SoA, if allowed send an ASnd frame or a non POWERLINK frame
                    (DllCsState::WaitSoa, DllCsEvent::Soa) => DllCsState::WaitSoc,
                    // Accept the PReq frame and send a PRes frame, report error DLL_CEV_LOSS_SOC and DLL_CEV_LOSS_SOA
                    (DllCsState::WaitSoa, DllCsEvent::Preq) => {
                        errors.push(DllError::LossOfSocThreshold);
                        errors.push(DllError::LossOfSoaThreshold);
                        DllCsState::WaitSoc
                    },
                    // Synchronise to the next SoC, report error DLL_CEV_LOSS_SOC and DLL_CEV_LOSS_SOA
                    (DllCsState::WaitSoa, DllCsEvent::SocTimeout) => {
                        errors.push(DllError::LossOfSocThreshold);
                        errors.push(DllError::LossOfSoaThreshold);
                        DllCsState::WaitSoc
                    },
                    // --- (DLL_CT04) ---
                    // Process frame
                    (DllCsState::WaitSoc, DllCsEvent::Asnd) => DllCsState::WaitSoc,
                    // Respond with PRes frame, report error DLL_CEV_LOSS_SOC
                    (DllCsState::WaitSoc, DllCsEvent::Preq) => {
                        errors.push(DllError::LossOfSocThreshold);
                        DllCsState::WaitSoc
                    },
                    // Report error DLL_CEV_LOSS_SOC
                    (DllCsState::WaitSoc, DllCsEvent::Pres) => {
                        errors.push(DllError::LossOfSocThreshold);
                        DllCsState::WaitSoc
                    },
                    // Report error DLL_CEV_LOSS_SOC
                    (DllCsState::WaitSoc, DllCsEvent::Soa) => {
                        errors.push(DllError::LossOfSocThreshold);
                        DllCsState::WaitSoc
                    },                    
                    // Report error DLL_CEV_LOSS_SOC
                    (DllCsState::WaitSoc, DllCsEvent::SocTimeout) => {
                        errors.push(DllError::LossOfSocThreshold);
                        DllCsState::WaitSoc
                    },
                    // --- (DLL_CT07) ---
                    // Process PRes frames (cross traffic)
                    (DllCsState::WaitPreq, DllCsEvent::Pres) => DllCsState::WaitPreq,
                    // Report error DLL_CEV_LOSS_SOA
                    (DllCsState::WaitPreq, DllCsEvent::Asnd) => {
                        errors.push(DllError::LossOfSoaThreshold);
                        DllCsState::WaitPreq
                    }, 
                    // Synchronise to the cycle begin, report error DLL_CEV_LOSS_SOA
                    (DllCsState::WaitPreq, DllCsEvent::Soc) => {
                        errors.push(DllError::LossOfSoaThreshold);
                        DllCsState::WaitPreq
                    }, 
                    // --- (DLL_CT08) ---
                    // Process SoA, if invited, transmit a legal Ethernet frame
                    (DllCsState::WaitPreq, DllCsEvent::Soa) => DllCsState::WaitSoc,
                    //  Synchronise on the next SoC, report error DLL_CEV_LOSS_SOC and DLL_CEV_LOSS_SOA
                    (DllCsState::WaitPreq, DllCsEvent::SocTimeout) => {
                        errors.push(DllError::LossOfSocThreshold);
                        errors.push(DllError::LossOfSoaThreshold);
                        DllCsState::WaitSoc
                    }, 
                    // --- (DLL_CT09) ---
                    // Synchronise on the SoC, report error DLL_CEV_LOSS_SOA
                    (DllCsState::WaitSoa, DllCsEvent::Soc) => {
                        errors.push(DllError::LossOfSoaThreshold);
                        DllCsState::WaitPreq
                    },
                    // --- (DLL_CT10) ---
                    // Process PRes frames (cross traffic)
                    (DllCsState::WaitSoa, DllCsEvent::Pres) => DllCsState::WaitSoa,
                    // Report error DLL_CEV_LOSS_SOA 
                    (DllCsState::WaitSoa, DllCsEvent::Asnd) => {
                        errors.push(DllError::LossOfSoaThreshold);
                        DllCsState::WaitSoa
                    },                 
                    // --- (DLL_CT01) ---
                    // A SoC can be received in any state and always resets the cycle to WaitPReq.
                    // Synchronise the start of cycle and generate a SoC trigger to the application
                    (_, DllCsEvent::Soc) => DllCsState::WaitPreq,

                    // If an unexpected event occurs, remain in the current state.
                    // Error reporting would be triggered here.
                    (current, _) => current,
                };
                self.state = next_state;
            },
            _ => {
                // In all other NMT states, the Dll state machine is considered non-cyclic.
                self.state = DllCsState::NonCyclic;
            }
        }
        if errors.is_empty() {
            None
        } else {
            Some(errors)
        }
    }

    /// Returns the current state of the DLL state machine.
    pub fn current_state(&self) -> DllCsState {
        self.state
    }
}

impl Default for DllCsStateMachine {
    fn default() -> Self {
        Self { state: DllCsState::NonCyclic }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dll_cn_happy_path_transitions() {
        let mut sm = DllCsStateMachine::new();
        let operational_state = NmtState::NmtCsOperational;

        // Initial state
        assert_eq!(sm.current_state(), DllCsState::NonCyclic);

        // A SoC starts the cycle
        sm.process_event(DllCsEvent::Soc, operational_state);
        assert_eq!(sm.current_state(), DllCsState::WaitPreq);

        // A PReq is received
        sm.process_event(DllCsEvent::Preq, operational_state);
        assert_eq!(sm.current_state(), DllCsState::WaitSoa);
        
        // An SoA ends the isochronous phase
        sm.process_event(DllCsEvent::Pres, operational_state);
        assert_eq!(sm.current_state(), DllCsState::WaitSoc);
        
        // A timeout resets the state machine
        sm.process_event(DllCsEvent::Soa, operational_state);
        assert_eq!(sm.current_state(), DllCsState::NonCyclic);
    }

    #[test]
    fn test_dll_cn_lost_frame_recovery() {
        let mut sm = DllCsStateMachine::new();
        let operational_state = NmtState::NmtCsOperational;
        
        // Start a cycle
        sm.process_event(DllCsEvent::Soc, operational_state);
        sm.process_event(DllCsEvent::Preq, operational_state);
        sm.process_event(DllCsEvent::Pres, operational_state);
        assert_eq!(sm.current_state(), DllCsState::WaitSoc);

        // SCENARIO: SoA from previous cycle and SoC from new cycle were lost.
        // The CN receives a PReq for the new cycle while still in WaitSoc.
        sm.process_event(DllCsEvent::Preq, operational_state);
        
        // The state machine should recover and move to WaitSoA.
        assert_eq!(sm.current_state(), DllCsState::WaitSoa);
    }
}