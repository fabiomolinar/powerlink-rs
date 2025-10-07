use crate::{frame::error::DllError, nmt::states::NmtState, NodeId};
use alloc::vec::Vec;

/// States for the Data Link Layer Cycle State Machine (DLL_MS) of a MN.
/// (Reference: EPSG DS 301, Section 4.2.4.6.2)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DllMsState {
    /// The cyclic communication is not active. Corresponds to `DLL_MS_NON_CYCLIC`.
    #[default]
    NonCyclic,
    /// Waits for the next cycle to begin. Corresponds to `DLL_MS_WAIT_SOC_TRIG`.
    WaitSocTrig,
    /// Waiting for a Poll Response (PRes) frame. Corresponds to `DLL_MS_WAIT_PRES`.
    WaitPres,
    /// Waits for the asynchronous phase to end. Corresponds to `DLL_MS_WAIT_ASND`.
    WaitAsnd,
    /// Waits for the asynchronous phase timeout or any Ethernet frame. Corresponds to `DLL_MS_WAIT_SOA`.
    WaitSoa,
}

/// Events that drive the DLL_MS, corresponding to internal triggers or received frames.
/// (Reference: EPSG DS 301, Section 4.2.4.6.3)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DllMsEvent {
    /// A PRes frame was received. Corresponds to `DLL_ME_PRES`.
    Pres,
    /// A PRes frame was not received in time. Corresponds to `DLL_ME_PRES_TIMEOUT`.
    PresTimeout,
    /// An ASnd frame or a non-POWERLINK frame was received. Corresponds to `DLL_ME_ASND`.
    Asnd,
    /// An ASnd frame was not received in time. Corresponds to `DLL_ME_ASND_TIMEOUT`.
    AsndTimeout,
    /// Triggers the emission of an SoC frame and starts a new cycle. Corresponds to `DLL_ME_SOC_TRIG`.
    SocTrig,
    /// Triggers a new reduced POWERLINK cycle. Corresponds to `DLL_ME_SOA_TRIG`.
    SoaTrig,
}

/// Manages the DLL cycle state for a Managing Node (MN).
pub struct DllMsStateMachine {
    state: DllMsState,
}

impl DllMsStateMachine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Processes an incoming event and transitions the state based on the current NMT state.
    /// The logic follows the state diagrams in Figure 31 and 32 of the specification.
    pub fn process_event(
        &mut self, event: DllMsEvent, nmt_state: NmtState, response_expected: bool, 
        async_in: bool, async_out: bool, isochr: bool, isochr_out: bool, dest_node_id: NodeId
    ) -> Option<Vec<DllError>> {
        let mut errors : Vec<DllError> = Vec::new();
        match nmt_state {
            NmtState::NmtPreOperational1 => {
                let next_state = match (self.state, event) {
                    (DllMsState::WaitSoa, DllMsEvent::SoaTrig) => {
                        match (async_in, response_expected, async_out) {
                            // --- DLL_MT10 ---
                            // Send SoA with Invite
                            (true, false, _) => DllMsState::WaitSoa,
                            // Send SoA with Invite to MN and send ASnd or non POWERLINK frame
                            (false, _, true) => DllMsState::WaitSoa,
                            // Send SoA
                            (false, _, false) => DllMsState::WaitSoa,
                            // --- DLL_MT11 ---
                            // Send SoA with Invite
                            (true, true, _) => DllMsState::WaitAsnd,
                        }
                    },                    
                    (DllMsState::WaitAsnd, e @ DllMsEvent::AsndTimeout | e @ DllMsEvent::SoaTrig) => {
                        if e == DllMsEvent::AsndTimeout {
                            errors.push(DllError::MevAsndTimeout);
                        }
                        match (async_in, response_expected) {
                            // --- DLL_MT12 ---
                            // Send SoA, ASnd if available
                            (false, _) => DllMsState::WaitSoa,
                            // Send SoA with Invite
                            (true, false) => DllMsState::WaitSoa,
                            // --- DLL_MT13 ---
                            // Send SoA with Invite, report error DLL_MEV_ASND_TIMEOUT
                            (true, true) => {
                                errors.push(DllError::MevAsndTimeout);
                                DllMsState::WaitAsnd
                            },
                        }
                    },
                    // If an unexpected event occurs, remain in the current state.
                    (current, _) => {
                        errors.push(DllError::UnexpectedEventInState);
                        current
                    },                    
                };
                self.state = next_state;
            },
            NmtState::NmtOperational | NmtState::NmtReadyToOperate | NmtState::NmtPreOperational2 => {
                let next_state = match (self.state, event) {
                    (DllMsState::WaitSocTrig, DllMsEvent::SocTrig) => {
                        match(isochr, async_in, isochr_out, async_out) {
                            // --- DLL_MT1 --- 
                            // Send SoC, PReq
                            (true, _, _, _) => DllMsState::WaitPres,
                            // --- DLL_MT6 --- 
                            // Send SoC, PRes and SoA with Invite
                            (false, true, true, _)  => DllMsState::WaitAsnd,
                            // Send SoC and SoA with Invite
                            (false, true, _, _)  => DllMsState::WaitAsnd,
                            // --- DLL_MT7 --- 
                            // Send SoC, PRes
                            (false, false, true, _) => DllMsState::WaitSocTrig,
                            // Send SoC, SoA and ASnd
                            (false, false, _, true) => DllMsState::WaitSocTrig,
                            // Send SoC
                            (false, false, _, _) => DllMsState::WaitSocTrig,
                        }
                    },
                    (DllMsState::WaitPres, e @ DllMsEvent::Pres | e @ DllMsEvent::PresTimeout) => {
                        if e == DllMsEvent::PresTimeout {
                            errors.push(DllError::LossOfPresThreshold { node_id: dest_node_id });
                        }
                        match(isochr, async_in, isochr_out, async_out) {
                            // --- DLL_MT2 --- Send next PReq
                            (true, _, _, _) => DllMsState::WaitPres,
                            // --- DLL_MT3 --- 
                            // Send PRes and SoA
                            (false, false, true, _) => DllMsState::WaitSocTrig,
                            // Send PRes and ASnd
                            (false, false, _, true) => DllMsState::WaitSocTrig,
                            // Send PRes
                            (false, false, _, _) => DllMsState::WaitSocTrig,
                            // --- DLL_MT4 --- 
                            // Send PRes and SoA with Invite
                            (false, true, true, _)  => DllMsState::WaitAsnd,
                            // Send PRes
                            (false, true, _, _)  => DllMsState::WaitAsnd,
                        }
                    },
                    (DllMsState::WaitAsnd, DllMsEvent::SocTrig) => {
                        match(isochr, async_in, isochr_out, async_out) {
                            // --- DLL_MT5 --- 
                            // Send SoC and PRes
                            (false, false, true, _) => DllMsState::WaitSocTrig,
                            // Send SoC and SoA and ASnd
                            (false, false, _, true) => DllMsState::WaitSocTrig,
                            // Send SoC
                            (false, false, _, _) => DllMsState::WaitSocTrig,
                            // --- DLL_MT8 ---
                            // Send SoC and SoA with Invite
                            (false, true, _, _) => DllMsState::WaitAsnd,                           
                            // --- DLL_MT9 ---
                            // Send SoC and PReq
                            (true, _, _, _)  => DllMsState::WaitPres,                            
                        }
                    },
                    // --- DLL_MT8 --- Process the frame
                    (DllMsState::WaitAsnd, DllMsEvent::Asnd) => DllMsState::WaitAsnd,

                    // --- DLL_MT0 --- Initial transition from NMT
                    (DllMsState::NonCyclic, DllMsEvent::SocTrig) => DllMsState::WaitSocTrig,

                    (current, _) => {
                        errors.push(DllError::UnexpectedEventInState);
                        current
                    },                  
                };
                self.state = next_state;
            },
            _ => {
                self.state = DllMsState::NonCyclic;
            }
        }        
        if errors.is_empty() {
            None
        } else {
            Some(errors)
        }
    }

    /// Returns the current state of the DLL state machine.
    pub fn current_state(&self) -> DllMsState {
        self.state
    }
}

impl Default for DllMsStateMachine {
    fn default() -> Self {
        Self { state: DllMsState::NonCyclic }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dll_ms_pre_operational_1_cycle() {
        let mut sm = DllMsStateMachine::new();
        let preop1_state = NmtState::NmtPreOperational1;
        
        // In PreOp1, the state machine should start in a state ready to send SoA.
        // For this test, we'll assume it's already in WAIT_SOA.
        sm.state = DllMsState::WaitSoa;

        // Event: Trigger SoA, expect a response from a CN.
        // isochr and async_out are false. async_in and response_expected are true.
        sm.process_event(
            DllMsEvent::SoaTrig, preop1_state, true, true, 
            false, false, false, NodeId(1), 
        );
        assert_eq!(sm.current_state(), DllMsState::WaitAsnd);

        // Event: Timeout waiting for ASnd. The MN should re-issue an SoA invite.
        // Since the next action is to re-invite, response_expected is true.
        sm.process_event(
            DllMsEvent::AsndTimeout, preop1_state, true,
             false, false, false, false, NodeId(1), 
        );
        assert_eq!(sm.current_state(), DllMsState::WaitSoa);
    }

    #[test]
    fn test_dll_ms_operational_happy_path() {
        let mut sm = DllMsStateMachine::new();
        let operational_state = NmtState::NmtOperational;

        // The machine starts in NonCyclic. An NMT state change would trigger the first SOC.
        assert_eq!(sm.current_state(), DllMsState::NonCyclic);
        
        // (DLL_MT0) NMT signals the start of the cyclic phase.
        sm.process_event(DllMsEvent::SocTrig, operational_state, false, false, false, false, false, NodeId(1),);
        assert_eq!(sm.current_state(), DllMsState::WaitSocTrig);

        // Event: A new cycle begins. MN sends SoC and first PReq.
        // isochr is true, indicating there are isochronous frames to send.
        sm.process_event(DllMsEvent::SocTrig, operational_state, false, false, false, true, false, NodeId(1), );
        assert_eq!(sm.current_state(), DllMsState::WaitPres);

        // Event: MN receives a PRes, sends the next PReq.
        // isochr is still true.
        sm.process_event(DllMsEvent::Pres, operational_state, false, false, false, true, false, NodeId(1), );
        assert_eq!(sm.current_state(), DllMsState::WaitPres);
        
        // Event: MN receives the last PRes. Isochronous phase is over. No async phase.
        // isochr is now false. async_in is false.
        sm.process_event(DllMsEvent::Pres, operational_state, false, false, false, false, false, NodeId(1), );
        assert_eq!(sm.current_state(), DllMsState::WaitSocTrig);
    }
}