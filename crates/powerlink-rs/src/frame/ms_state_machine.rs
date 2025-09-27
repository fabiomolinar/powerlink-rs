#![allow(non_camel_case_types)]

use crate::nmt::states::NMTState;

/// States for the Data Link Layer Cycle State Machine (DLL_MS) of a MN.
/// (EPSG DS 301, Section 4.2.4.6.2)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DllMsState {
    /// The cyclic communication is not active.
    #[default]
    DLL_MS_NON_CYCLIC,
    /// Remains in this state until the next cycle begins with a DLL_ME_SOC_TRIG.
    DLL_MS_WAIT_SOC_TRIG,
    /// Waiting for a Poll Response (PRes) frame.
    DLL_MS_WAIT_PRES,
    /// Waits in this state until the asynchronous phase ends with the event DLL_ME_SOC_TRIG.
    DLL_MS_WAIT_ASND,
    // Wait in this state until the timeout of the async phase elapsed or any Ethernet frame was received. 
    DLL_MS_WAIT_SOA,
}

/// Events that drive the DLL_MS, corresponding to internal triggers or received frames.
/// (EPSG DS 301, Section 4.2.4.6.3)
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
        &mut self, event: DllMsEvent, nmt_state: NMTState, response_expected: bool, 
        async_in: bool, async_out: bool, isochr: bool, isochr_out: bool
    ) {
        match nmt_state {
            NMTState::NMT_MS_PRE_OPERATIONAL_1 => {
                let next_state = match (self.state, event) {
                    (DllMsState::DLL_MS_WAIT_SOA, DllMsEvent::DLL_ME_SOA_TRIG) => {
                        match (async_in, response_expected, async_out) {
                            // --- DLL_MT10 ---
                            // Send SoA with Invite
                            (true, false, _) => DllMsState::DLL_MS_WAIT_SOA,
                            // Send SoA with Invite to MN and send ASnd or non POWERLINK frame
                            (false, _, true) => DllMsState::DLL_MS_WAIT_SOA,
                            // Send SoA
                            (false, _, false) => DllMsState::DLL_MS_WAIT_SOA,
                            // --- DLL_MT11 ---
                            // Send SoA with Invite
                            (true, true, _) => DllMsState::DLL_MS_WAIT_ASND,
                        }
                    },                    
                    (DllMsState::DLL_MS_WAIT_ASND, DllMsEvent::DLL_ME_ASND_TIMEOUT | DllMsEvent::DLL_ME_SOA_TRIG) => {
                        match (async_in, response_expected) {
                            // --- DLL_MT12 ---
                            // Send SoA, ASnd if available
                            (false, _) => DllMsState::DLL_MS_WAIT_SOA,
                            // Send SoA with Invite
                            (true, false) => DllMsState::DLL_MS_WAIT_SOA,
                            // --- DLL_MT13 ---
                            // Send SoA with Invite, report error DLL_MEV_ASND_TIMEOUT
                            (true, true) => DllMsState::DLL_MS_WAIT_ASND,
                        }
                    },
                    // If an unexpected event occurs, remain in the current state.
                    (current, _) => current,                    
                };
                self.state = next_state;
            },
            NMTState::NMT_MS_OPERATIONAL | NMTState::NMT_MS_READY_TO_OPERATE | NMTState::NMT_MS_PRE_OPERATIONAL_2 => {
                let next_state = match (self.state, event) {
                    (DllMsState::DLL_MS_WAIT_SOC_TRIG, DllMsEvent::DLL_ME_SOC_TRIG) => {
                        match(isochr, async_in, isochr_out, async_out) {
                            // --- DLL_MT1 --- 
                            // Send SoC, PReq
                            (true, _, _, _) => DllMsState::DLL_MS_WAIT_PRES,
                            // --- DLL_MT6 --- 
                            // Send SoC, PRes and SoA with Invite
                            (false, true, true, _)  => DllMsState::DLL_MS_WAIT_ASND,
                            // Send SoC and SoA with Invite
                            (false, true, _, _)  => DllMsState::DLL_MS_WAIT_ASND,
                            // --- DLL_MT7 --- 
                            // Send SoC, PRes
                            (false, false, true, _) => DllMsState::DLL_MS_WAIT_SOC_TRIG,
                            // Send SoC, SoA and ASnd
                            (false, false, _, true) => DllMsState::DLL_MS_WAIT_SOC_TRIG,
                            // Send SoC
                            (false, false, _, _) => DllMsState::DLL_MS_WAIT_SOC_TRIG,
                        }
                    },
                    (DllMsState::DLL_MS_WAIT_PRES, e @ DllMsEvent::DLL_ME_PRES | e @ DllMsEvent::DLL_ME_PRES_TIMEOUT) => {
                        if e == DllMsEvent::DLL_ME_PRES_TIMEOUT {
                            // TODO:
                            // Here, error reporting for DLL_MEV_PRES_TIMEOUT would be triggered.
                        }
                        match(isochr, async_in, isochr_out, async_out) {
                            // --- DLL_MT2 --- Send next PReq
                            (true, _, _, _) => DllMsState::DLL_MS_WAIT_PRES,
                            // --- DLL_MT3 --- 
                            // Send PRes and SoA
                            (false, false, true, _) => DllMsState::DLL_MS_WAIT_SOC_TRIG,
                            // Send PRes and ASnd
                            (false, false, _, true) => DllMsState::DLL_MS_WAIT_SOC_TRIG,
                            // Send PRes
                            (false, false, _, _) => DllMsState::DLL_MS_WAIT_SOC_TRIG,
                            // --- DLL_MT4 --- 
                            // Send PRes and SoA with Invite
                            (false, true, true, _)  => DllMsState::DLL_MS_WAIT_ASND,
                            // Send PRes
                            (false, true, _, _)  => DllMsState::DLL_MS_WAIT_ASND,
                        }
                    },
                    (DllMsState::DLL_MS_WAIT_ASND, DllMsEvent::DLL_ME_SOC_TRIG) => {
                        match(isochr, async_in, isochr_out, async_out) {
                            // --- DLL_MT5 --- 
                            // Send SoC and PRes
                            (false, false, true, _) => DllMsState::DLL_MS_WAIT_SOC_TRIG,
                            // Send SoC and SoA and ASnd
                            (false, false, _, true) => DllMsState::DLL_MS_WAIT_SOC_TRIG,
                            // Send SoC
                            (false, false, _, _) => DllMsState::DLL_MS_WAIT_SOC_TRIG,
                            // --- DLL_MT8 ---
                            // Send SoC and SoA with Invite
                            (false, true, _, _) => DllMsState::DLL_MS_WAIT_ASND,                           
                            // --- DLL_MT9 ---
                            // Send SoC and PReq
                            (true, _, _, _)  => DllMsState::DLL_MS_WAIT_PRES,                            
                        }
                    },
                    // --- DLL_MT8 --- Process the frame
                    (DllMsState::DLL_MS_WAIT_ASND, DllMsEvent::DLL_ME_ASND) => DllMsState::DLL_MS_WAIT_ASND,

                    // --- DLL_MT0 --- Initial transition from NMT
                    (DllMsState::DLL_MS_NON_CYCLIC, DllMsEvent::DLL_ME_SOC_TRIG) => DllMsState::DLL_MS_WAIT_SOC_TRIG,

                    (current, _) => current,                  
                };
                self.state = next_state;
            },
            _ => {
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
    fn test_dll_ms_pre_operational_1_cycle() {
        let mut sm = DllMsStateMachine::new();
        let preop1_state = NMTState::NMT_MS_PRE_OPERATIONAL_1;
        
        // In PreOp1, the state machine should start in a state ready to send SoA.
        // For this test, we'll assume it's already in WAIT_SOA.
        sm.state = DllMsState::DLL_MS_WAIT_SOA;

        // Event: Trigger SoA, expect a response from a CN.
        // isochr and async_out are false. async_in and response_expected are true.
        sm.process_event(DllMsEvent::DLL_ME_SOA_TRIG, preop1_state, true, true, false, false, false);
        assert_eq!(sm.current_state(), DllMsState::DLL_MS_WAIT_ASND);

        // Event: Timeout waiting for ASnd. The MN should re-issue an SoA invite.
        // Since the next action is to re-invite, response_expected is true.
        sm.process_event(DllMsEvent::DLL_ME_ASND_TIMEOUT, preop1_state, true, false, false, false, false);
        assert_eq!(sm.current_state(), DllMsState::DLL_MS_WAIT_ASND);
    }

    #[test]
    fn test_dll_ms_operational_happy_path() {
        let mut sm = DllMsStateMachine::new();
        let operational_state = NMTState::NMT_MS_OPERATIONAL;

        // The machine starts in NonCyclic. An NMT state change would trigger the first SOC.
        assert_eq!(sm.current_state(), DllMsState::DLL_MS_NON_CYCLIC);
        
        // (DLL_MT0) NMT signals the start of the cyclic phase.
        sm.process_event(DllMsEvent::DLL_ME_SOC_TRIG, operational_state, false, false, false, false, false);
        assert_eq!(sm.current_state(), DllMsState::DLL_MS_WAIT_SOC_TRIG);

        // Event: A new cycle begins. MN sends SoC and first PReq.
        // isochr is true, indicating there are isochronous frames to send.
        sm.process_event(DllMsEvent::DLL_ME_SOC_TRIG, operational_state, false, false, false, true, false);
        assert_eq!(sm.current_state(), DllMsState::DLL_MS_WAIT_PRES);

        // Event: MN receives a PRes, sends the next PReq.
        // isochr is still true.
        sm.process_event(DllMsEvent::DLL_ME_PRES, operational_state, false, false, false, true, false);
        assert_eq!(sm.current_state(), DllMsState::DLL_MS_WAIT_PRES);
        
        // Event: MN receives the last PRes. Isochronous phase is over. No async phase.
        // isochr is now false. async_in is false.
        sm.process_event(DllMsEvent::DLL_ME_PRES, operational_state, false, false, false, false, false);
        assert_eq!(sm.current_state(), DllMsState::DLL_MS_WAIT_SOC_TRIG);
    }
}