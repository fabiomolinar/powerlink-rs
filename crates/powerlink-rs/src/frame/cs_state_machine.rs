use crate::frame::error::DllError;
use crate::nmt::states::NmtState;
use alloc::vec::Vec;
use log::debug;

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
    pub fn process_event(
        &mut self,
        event: DllCsEvent,
        nmt_state: NmtState,
    ) -> Option<Vec<DllError>> {
        debug!(
            "DLL_CS processing event {:?} in state {:?} (NMT state: {:?})",
            event, self.state, nmt_state
        );
        let mut errors: Vec<DllError> = Vec::new();
        // The DLL_CS is active only in specific NMT states.
        match nmt_state {
            NmtState::NmtPreOperational2
            | NmtState::NmtReadyToOperate
            | NmtState::NmtOperational
            | NmtState::NmtCsStopped => {
                let next_state = match (self.state, event) {
                    // --- (DLL_CT02) ---
                    // Process the PReq frame and send a PRes frame
                    (DllCsState::WaitPreq, DllCsEvent::Preq) => DllCsState::WaitSoa,
                    // --- (DLL_CT03) ---
                    // Process SoA, if allowed send an ASnd frame or a non POWERLINK frame
                    (DllCsState::WaitSoa, DllCsEvent::Soa) => DllCsState::WaitSoc,
                    // Accept the PReq frame and send a PRes frame, report error DLL_CEV_LOSS_SOC and DLL_CEV_LOSS_SOA
                    (DllCsState::WaitSoa, DllCsEvent::Preq) => {
                        errors.push(DllError::LossOfSoc);
                        errors.push(DllError::LossOfSoa);
                        DllCsState::WaitSoc
                    }
                    // Synchronise to the next SoC, report error DLL_CEV_LOSS_SOC and DLL_CEV_LOSS_SOA
                    (DllCsState::WaitSoa, DllCsEvent::SocTimeout) => {
                        errors.push(DllError::LossOfSoc);
                        errors.push(DllError::LossOfSoa);
                        DllCsState::WaitSoc
                    }
                    // --- (DLL_CT04) ---
                    // Process frame
                    (DllCsState::WaitSoc, DllCsEvent::Asnd) => DllCsState::WaitSoc,
                    // Respond with PRes frame, report error DLL_CEV_LOSS_SOC
                    (DllCsState::WaitSoc, DllCsEvent::Preq) => {
                        errors.push(DllError::LossOfSoc);
                        DllCsState::WaitSoc
                    }
                    // Report error DLL_CEV_LOSS_SOC
                    (DllCsState::WaitSoc, DllCsEvent::Pres) => {
                        errors.push(DllError::LossOfSoc);
                        DllCsState::WaitSoc
                    }
                    // Report error DLL_CEV_LOSS_SOC
                    (DllCsState::WaitSoc, DllCsEvent::Soa) => {
                        errors.push(DllError::LossOfSoc);
                        DllCsState::WaitSoc
                    }
                    // Report error DLL_CEV_LOSS_SOC
                    (DllCsState::WaitSoc, DllCsEvent::SocTimeout) => {
                        errors.push(DllError::LossOfSoc);
                        DllCsState::WaitSoc
                    }
                    // --- (DLL_CT07) ---
                    // Process PRes frames (cross traffic)
                    (DllCsState::WaitPreq, DllCsEvent::Pres) => DllCsState::WaitPreq,
                    // Report error DLL_CEV_LOSS_SOA
                    (DllCsState::WaitPreq, DllCsEvent::Asnd) => {
                        errors.push(DllError::LossOfSoa);
                        DllCsState::WaitPreq
                    }
                    // Synchronise to the cycle begin, report error DLL_CEV_LOSS_SOA
                    (DllCsState::WaitPreq, DllCsEvent::Soc) => {
                        errors.push(DllError::LossOfSoa);
                        DllCsState::WaitPreq
                    }
                    // --- (DLL_CT08) ---
                    // Process SoA, if invited, transmit a legal Ethernet frame
                    (DllCsState::WaitPreq, DllCsEvent::Soa) => {
                        errors.push(DllError::LossOfPreq);
                        DllCsState::WaitSoc
                    }
                    //  Synchronise on the next SoC, report error DLL_CEV_LOSS_SOC and DLL_CEV_LOSS_SOA
                    (DllCsState::WaitPreq, DllCsEvent::SocTimeout) => {
                        errors.push(DllError::LossOfSoc);
                        errors.push(DllError::LossOfSoa);
                        DllCsState::WaitSoc
                    }
                    // --- (DLL_CT09) ---
                    // Synchronise on the SoC, report error DLL_CEV_LOSS_SOA
                    (DllCsState::WaitSoa, DllCsEvent::Soc) => {
                        errors.push(DllError::LossOfSoa);
                        DllCsState::WaitPreq
                    }
                    // --- (DLL_CT10) ---
                    // Process PRes frames (cross traffic)
                    (DllCsState::WaitSoa, DllCsEvent::Pres) => DllCsState::WaitSoa,
                    // Report error DLL_CEV_LOSS_SOA
                    (DllCsState::WaitSoa, DllCsEvent::Asnd) => {
                        errors.push(DllError::LossOfSoa);
                        DllCsState::WaitSoa
                    }
                    // --- (DLL_CT01) ---
                    // A SoC can be received in any state and always resets the cycle to WaitPReq.
                    // Synchronise the start of cycle and generate a SoC trigger to the application
                    (_, DllCsEvent::Soc) => DllCsState::WaitPreq,

                    // If an unexpected event occurs, remain in the current state.
                    // Error reporting would be triggered here.
                    (current, _) => {
                        errors.push(DllError::UnexpectedEventInState {
                            state: current as u8,
                            event: event as u8,
                        });
                        current
                    }
                };
                self.state = next_state;
            }
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
        Self {
            state: DllCsState::NonCyclic,
        }
    }
}

#[cfg(test)]
#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn test_happy_path() {
        let mut sm = DllCsStateMachine::new();
        let op_state = NmtState::NmtOperational;
        assert_eq!(sm.current_state(), DllCsState::NonCyclic);
        assert!(sm.process_event(DllCsEvent::Soc, op_state).is_none());
        assert_eq!(sm.current_state(), DllCsState::WaitPreq);
        assert!(sm.process_event(DllCsEvent::Preq, op_state).is_none());
        assert_eq!(sm.current_state(), DllCsState::WaitSoa);
        assert!(sm.process_event(DllCsEvent::Soa, op_state).is_none());
        assert_eq!(sm.current_state(), DllCsState::WaitSoc);
    }

    #[test]
    fn test_lost_preq() {
        let mut sm = DllCsStateMachine::new();
        let op_state = NmtState::NmtOperational;
        sm.process_event(DllCsEvent::Soc, op_state); // -> WaitPreq
        let errors = sm.process_event(DllCsEvent::Soa, op_state);
        assert_eq!(errors, Some(vec![DllError::LossOfPreq]));
        assert_eq!(sm.current_state(), DllCsState::WaitSoc);
    }

    #[test]
    fn test_lost_soa_and_soc() {
        let mut sm = DllCsStateMachine::new();
        let op_state = NmtState::NmtOperational;
        sm.process_event(DllCsEvent::Soc, op_state);
        sm.process_event(DllCsEvent::Preq, op_state);
        sm.process_event(DllCsEvent::Soa, op_state); // -> WaitSoc
        let errors = sm.process_event(DllCsEvent::Preq, op_state);
        assert_eq!(errors, Some(vec![DllError::LossOfSoc]));
        assert_eq!(sm.current_state(), DllCsState::WaitSoc);
    }

    #[test]
    fn test_soc_timeout() {
        let mut sm = DllCsStateMachine::new();
        let op_state = NmtState::NmtOperational;
        sm.process_event(DllCsEvent::Soc, op_state); // -> WaitPreq
        let errors = sm.process_event(DllCsEvent::SocTimeout, op_state);
        assert_eq!(errors, Some(vec![DllError::LossOfSoc, DllError::LossOfSoa]));
        assert_eq!(sm.current_state(), DllCsState::WaitSoc);
    }
}
