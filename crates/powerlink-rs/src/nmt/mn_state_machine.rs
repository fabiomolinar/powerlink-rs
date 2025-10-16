// In crates/powerlink-rs/src/nmt/mn_state_machine.rs

use super::state_machine::NmtStateMachine;
use crate::frame::DllError;
use crate::od::{ObjectDictionary, ObjectValue};
use crate::types::{NodeId, C_ADR_MN_DEF_NODE_ID};
use crate::PowerlinkError;
use super::flags::FeatureFlags;
use super::states::{NmtEvent, NmtState};
use alloc::vec::Vec;

/// Manages the NMT state for a Managing Node.
pub struct MnNmtStateMachine {
    pub current_state: NmtState,
    pub node_id: NodeId,
    pub feature_flags: FeatureFlags,
    pub wait_not_active_timeout: u32,
    pub startup_flags: u32,
}

impl MnNmtStateMachine {
    /// Creates a new MN NMT state machine with pre-validated parameters.
    pub fn new(
        node_id: NodeId,
        feature_flags: FeatureFlags,
        wait_not_active_timeout: u32,
        startup_flags: u32,
    ) -> Self {
        Self {
            current_state: NmtState::NmtGsInitialising,
            node_id,
            feature_flags,
            wait_not_active_timeout,
            startup_flags,
        }
    }

    /// A fallible constructor that reads its configuration from an Object Dictionary.
    pub fn from_od(od: &ObjectDictionary) -> Result<Self, PowerlinkError> {
        // Feature Flags from OD entry 0x1F82, sub-index 0.
        let feature_flags_val = od.read(0x1F82, 0).ok_or(PowerlinkError::ObjectNotFound)?;
        let feature_flags = if let ObjectValue::Unsigned32(val) = &*feature_flags_val {
            FeatureFlags::from_bits_truncate(*val)
        } else {
            return Err(PowerlinkError::TypeMismatch);
        };

        // WaitNotActive timeout from OD entry 0x1F89, sub-index 1.
        let wait_not_active_timeout_val = od.read(0x1F89, 1).ok_or(PowerlinkError::ObjectNotFound)?;
        let wait_not_active_timeout = if let ObjectValue::Unsigned32(val) = &*wait_not_active_timeout_val {
            *val
        } else {
            return Err(PowerlinkError::TypeMismatch);
        };
        
        // NMT_StartUp_U32 from OD entry 0x1F80, sub-index 0.
        let startup_flags_val = od.read(0x1F80, 0).ok_or(PowerlinkError::ObjectNotFound)?;
        let startup_flags = if let ObjectValue::Unsigned32(val) = &*startup_flags_val {
            *val
        } else {
            return Err(PowerlinkError::TypeMismatch);
        };

        Ok(Self::new(
            NodeId(C_ADR_MN_DEF_NODE_ID),
            feature_flags,
            wait_not_active_timeout,
            startup_flags,
        ))
    }
}

impl NmtStateMachine for MnNmtStateMachine {
    fn current_state(&self) -> NmtState {
        self.current_state
    }

    fn reset(&mut self, event: NmtEvent) {
         match event {
            NmtEvent::Reset => self.current_state = NmtState::NmtGsInitialising,
            NmtEvent::ResetNode => self.current_state = NmtState::NmtGsResetApplication,
            NmtEvent::ResetCommunication => self.current_state = NmtState::NmtGsResetCommunication,
            NmtEvent::ResetConfiguration => self.current_state = NmtState::NmtGsResetConfiguration,
            _ => {}, // Ignore other events
        }
    }

    fn run_internal_initialisation(&mut self, od: &mut ObjectDictionary) {
        // The MN follows the same initial reset sequence as the CN.
        if self.current_state == NmtState::NmtGsInitialising {
            self.current_state = NmtState::NmtGsResetApplication;
            self.update_od_state(od);
            self.current_state = NmtState::NmtGsResetCommunication;
            self.update_od_state(od);
            self.current_state = NmtState::NmtGsResetConfiguration;
            self.update_od_state(od);
            self.current_state = NmtState::NmtNotActive;
            self.update_od_state(od);
        }
    }

    /// Processes an external event and transitions the NMT state accordingly.
    /// The logic follows the MN state diagram (Figure 73) from the specification.
    fn process_event(&mut self, event: NmtEvent, od: &mut ObjectDictionary) -> Option<Vec<DllError>> {
        let mut errors: Vec<DllError> = Vec::new();
        let old_state = self.current_state;
        let next_state = match (self.current_state, event) {
            // --- Reset and Initialisation Transitions (Same as CN) ---
            (_, NmtEvent::Reset) => NmtState::NmtGsInitialising,
            (_, NmtEvent::ResetNode) => NmtState::NmtGsResetApplication,
            (_, NmtEvent::ResetCommunication) => NmtState::NmtGsResetCommunication,
            (_, NmtEvent::ResetConfiguration) => NmtState::NmtGsResetConfiguration,

            // --- MN Boot-up Sequence ---
            
            (NmtState::NmtNotActive, NmtEvent::Timeout) => {
                // Check NMT_StartUp_U32.Bit13
                if (self.startup_flags & (1 << 13)) != 0 {
                    // (NMT_MT7) Go to BasicEthernet
                    NmtState::NmtBasicEthernet
                } else {
                    // (NMT_MT2) Go to PreOp1
                    NmtState::NmtPreOperational1
                }
            },
            
            // (NMT_MT3) Once all mandatory CNs are identified, start the full EPL cycle.
            (NmtState::NmtPreOperational1, NmtEvent::AllCnsIdentified) => NmtState::NmtPreOperational2,

            // (NMT_MT4) Once MN config is done and all mandatory CNs are ready, move to ReadyToOperate.
            (NmtState::NmtPreOperational2, NmtEvent::ConfigurationCompleteCnsReady) => NmtState::NmtReadyToOperate,

            // (NMT_MT5) Once isochronous communication is stable, enter the final operational state.
            (NmtState::NmtReadyToOperate, NmtEvent::StartNode) => NmtState::NmtOperational,

            // --- Operational State Transitions ---
            
            // (NMT_MT6) A critical error (e.g., mandatory CN lost) forces a reset to PreOp1.
            (NmtState::NmtOperational, NmtEvent::Error) => NmtState::NmtPreOperational1,

            // If no specific transition is defined, remain in the current state.
            (current, _) => {
                errors.push(DllError::UnexpectedEventInState { state: current as u8, event: event as u8 });
                current
            },
        };

        if old_state != next_state {
            self.current_state = next_state;
            self.update_od_state(od);
        }
        
        if errors.is_empty() {
            None
        } else {
            Some(errors)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::od::{Object, ObjectEntry, AccessType};
    use alloc::vec;

    fn get_test_mn_od() -> ObjectDictionary<'static> {
        let mut od = ObjectDictionary::new(None);
        let flags = FeatureFlags::ISOCHRONOUS | FeatureFlags::SDO_ASND | FeatureFlags::SDO_UDP;
        od.insert(0x1F82, ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned32(flags.0)),
            name: "NMT_FeatureFlags_U32",
            access: AccessType::Constant,
        });
        od.insert(0x1F80, ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned32(0)), // Default: Bit 13 is 0
            name: "NMT_StartUp_U32",
            access: AccessType::ReadWrite,
        });
        od.insert(0x1F89, ObjectEntry {
            object: Object::Record(vec![
                ObjectValue::Unsigned32(1_000_000), // MNWaitNotAct_U32
                ObjectValue::Unsigned32(500_000),  // MNTimeoutPreOp1_U32
            ]),
            name: "NMT_BootTime_REC",
            access: AccessType::ReadWrite,
        });
         od.insert(0x1F8C, ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned8(0)),
            name: "NMT_CurrNMTState_U8",
            access: AccessType::ReadOnly,
        });
        od
    }
    
    #[test]
    fn test_mn_from_od_reads_parameters() {
        let od = get_test_mn_od();
        let nmt = MnNmtStateMachine::from_od(&od).unwrap();
        assert_eq!(nmt.node_id, NodeId(C_ADR_MN_DEF_NODE_ID));
        assert!(nmt.feature_flags.contains(FeatureFlags::SDO_UDP));
        assert_eq!(nmt.wait_not_active_timeout, 1_000_000);
        assert_eq!(nmt.startup_flags, 0);
    }
    
    #[test]
    fn test_mn_boot_up_happy_path() {
        let mut od = get_test_mn_od();
        let mut nmt = MnNmtStateMachine::from_od(&od).unwrap();
        
        // Assume initial state is NotActive after internal initialization
        nmt.current_state = NmtState::NmtNotActive;
        
        // NMT_MT2
        nmt.process_event(NmtEvent::Timeout, &mut od);
        assert_eq!(nmt.current_state(), NmtState::NmtPreOperational1);
        assert_eq!(od.read_u8(0x1F8C, 0), Some(NmtState::NmtPreOperational1 as u8));
        
        // NMT_MT3
        nmt.process_event(NmtEvent::AllCnsIdentified, &mut od);
        assert_eq!(nmt.current_state(), NmtState::NmtPreOperational2);
        
        // NMT_MT4
        nmt.process_event(NmtEvent::ConfigurationCompleteCnsReady, &mut od);
        assert_eq!(nmt.current_state(), NmtState::NmtReadyToOperate);
        
        // NMT_MT5
        nmt.process_event(NmtEvent::StartNode, &mut od);
        assert_eq!(nmt.current_state(), NmtState::NmtOperational);
        assert_eq!(od.read_u8(0x1F8C, 0), Some(NmtState::NmtOperational as u8));
    }

    #[test]
    fn test_mn_boot_to_basic_ethernet() {
        let mut od = get_test_mn_od();
        // Set bit 13 in NMT_StartUp_U32
        od.write(0x1F80, 0, ObjectValue::Unsigned32(1 << 13)).unwrap();
        let mut nmt = MnNmtStateMachine::from_od(&od).unwrap();
        
        nmt.current_state = NmtState::NmtNotActive;
        
        // NMT_MT7
        nmt.process_event(NmtEvent::Timeout, &mut od);
        assert_eq!(nmt.current_state(), NmtState::NmtBasicEthernet);
        assert_eq!(od.read_u8(0x1F8C, 0), Some(NmtState::NmtBasicEthernet as u8));
    }
    
    #[test]
    fn test_mn_error_handling_transition() {
        let mut od = get_test_mn_od();
        let mut nmt = MnNmtStateMachine::from_od(&od).unwrap();
        nmt.current_state = NmtState::NmtOperational;
        
        // NMT_MT6
        nmt.process_event(NmtEvent::Error, &mut od);
        assert_eq!(nmt.current_state(), NmtState::NmtPreOperational1);
        assert_eq!(od.read_u8(0x1F8C, 0), Some(NmtState::NmtPreOperational1 as u8));
    }
}

