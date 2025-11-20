// crates/powerlink-rs/src/nmt/mn_state_machine.rs
use super::flags::FeatureFlags;
use super::state_machine::NmtStateMachine;
use super::states::NmtState;
use crate::PowerlinkError;
use crate::frame::DllError;
use crate::nmt::events::NmtEvent;
use crate::od::{ObjectDictionary, ObjectValue};
use crate::types::{C_ADR_MN_DEF_NODE_ID, NodeId};
use alloc::vec::Vec;
use log::{debug, info, warn};

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
        debug!("Initializing MN NMT state machine from Object Dictionary.");
        // Feature Flags from OD entry 0x1F82, sub-index 0.
        let feature_flags_val = od.read(0x1F82, 0).ok_or(PowerlinkError::ObjectNotFound)?;
        let feature_flags = if let ObjectValue::Unsigned32(val) = &*feature_flags_val {
            FeatureFlags::from_bits_truncate(*val)
        } else {
            return Err(PowerlinkError::TypeMismatch);
        };

        // WaitNotActive timeout from OD entry 0x1F89, sub-index 1.
        let wait_not_active_timeout_val =
            od.read(0x1F89, 1).ok_or(PowerlinkError::ObjectNotFound)?;
        let wait_not_active_timeout =
            if let ObjectValue::Unsigned32(val) = &*wait_not_active_timeout_val {
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

        info!(
            "MN NMT configured with FeatureFlags: {:?}, WaitNotActiveTimeout: {}, StartupFlags: {:#010x}",
            feature_flags, wait_not_active_timeout, startup_flags
        );

        Ok(Self::new(
            NodeId(C_ADR_MN_DEF_NODE_ID),
            feature_flags,
            wait_not_active_timeout,
            startup_flags,
        ))
    }
}

impl NmtStateMachine for MnNmtStateMachine {
    fn node_id(&self) -> NodeId {
        self.node_id
    }

    fn current_state(&self) -> NmtState {
        self.current_state
    }

    fn set_state(&mut self, new_state: NmtState) {
        self.current_state = new_state;
    }

    /// Processes an external event and transitions the NMT state accordingly.
    /// The logic follows the MN state diagram (Figure 73) from the specification.
    fn process_event(
        &mut self,
        event: NmtEvent,
        od: &mut ObjectDictionary,
    ) -> Option<Vec<DllError>> {
        let errors: Vec<DllError> = Vec::new();
        let old_state = self.current_state;

        // --- Handle Common Reset Events ---
        if matches!(
            event,
            NmtEvent::Reset
                | NmtEvent::SwReset
                | NmtEvent::ResetNode
                | NmtEvent::ResetCommunication
                | NmtEvent::ResetConfiguration
        ) {
            self.reset(event, od); // Pass OD to reset
            if old_state != self.current_state {
                self.update_od_state(od);
            }
            // After a reset, a full re-initialisation sequence should run.
            // Note: reset() now handles the full cascade down to NotActive.
            return None;
        }

        let next_state = match (self.current_state, event) {
            // --- MN Boot-up Sequence (Figure 73) ---

            // (NMT_MT2 / NMT_MT7) Timeout in NotActive
            (NmtState::NmtNotActive, NmtEvent::Timeout) => {
                // Check NMT_StartUp_U32.Bit13 (0x1F80)
                if (self.startup_flags & (1 << 13)) != 0 {
                    // (NMT_MT7) Go to BasicEthernet
                    NmtState::NmtBasicEthernet
                } else {
                    // (NMT_MT2) Go to PreOp1
                    NmtState::NmtPreOperational1
                }
            }

            // (NMT_MT3) All mandatory CNs identified
            (NmtState::NmtPreOperational1, NmtEvent::AllCnsIdentified) => {
                NmtState::NmtPreOperational2
            }

            // (NMT_MT4) MN configuration complete and all mandatory CNs ready to operate
            (NmtState::NmtPreOperational2, NmtEvent::ConfigurationCompleteCnsReady) => {
                NmtState::NmtReadyToOperate
            }

            // (NMT_MT5) All mandatory CNs are confirmed to be in the Operational state.
            (NmtState::NmtReadyToOperate, NmtEvent::AllMandatoryCnsOperational) => {
                NmtState::NmtOperational
            }

            // --- Operational State Transitions ---

            // (NMT_MT6) A critical error (e.g., mandatory CN lost) forces a reset to PreOp1.
            (NmtState::NmtOperational, NmtEvent::Error) => NmtState::NmtPreOperational1,

            // (NMT_CT12) MN in BasicEthernet detects other POWERLINK traffic
            (NmtState::NmtBasicEthernet, NmtEvent::PowerlinkFrameReceived) => {
                NmtState::NmtPreOperational1
            }

            // If no specific transition is defined, remain in the current state.
            (current, _) => {
                warn!("[NMT] Unhandled event {:?} in state {:?}", event, current);
                current
            }
        };

        if old_state != next_state {
            info!(
                "MN NMT state transition: {:?} -> {:?} (on event: {:?})",
                old_state, next_state, event
            );
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
    use crate::od::{AccessType, Object, ObjectEntry};
    use alloc::vec;

    fn get_test_mn_od() -> ObjectDictionary<'static> {
        let mut od = ObjectDictionary::new(None);
        let flags = FeatureFlags::ISOCHRONOUS | FeatureFlags::SDO_ASND | FeatureFlags::SDO_UDP;
        od.insert(
            0x1F82,
            ObjectEntry {
                object: Object::Variable(ObjectValue::Unsigned32(flags.0)),
                name: "NMT_FeatureFlags_U32",
                access: Some(AccessType::Constant),
                default_value: Some(ObjectValue::Unsigned32(flags.0)),
                value_range: None,
                pdo_mapping: None,
                category: crate::od::Category::Optional,
            },
        );
        od.insert(
            0x1F80,
            ObjectEntry {
                object: Object::Variable(ObjectValue::Unsigned32(0)), // Default: Bit 13 is 0
                name: "NMT_StartUp_U32",
                access: Some(AccessType::ReadWrite),
                default_value: None,
                value_range: None,
                pdo_mapping: None,
                category: crate::od::Category::Optional,
            },
        );
        od.insert(
            0x1F89,
            ObjectEntry {
                object: Object::Record(vec![
                    ObjectValue::Unsigned32(1_000_000), // MNWaitNotAct_U32 (1 sec)
                    ObjectValue::Unsigned32(500_000),   // MNTimeoutPreOp1_U32 (500 ms)
                ]),
                name: "NMT_BootTime_REC",
                access: Some(AccessType::ReadWrite),
                default_value: None,
                value_range: None,
                pdo_mapping: None,
                category: crate::od::Category::Optional,
            },
        );
        od.insert(
            0x1F8C,
            ObjectEntry {
                object: Object::Variable(ObjectValue::Unsigned8(0)),
                name: "NMT_CurrNMTState_U8",
                access: Some(AccessType::ReadOnly),
                default_value: None,
                value_range: None,
                pdo_mapping: None,
                category: crate::od::Category::Optional,
            },
        );
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
        assert_eq!(
            od.read_u8(0x1F8C, 0),
            Some(NmtState::NmtPreOperational1 as u8)
        );

        // NMT_MT3
        nmt.process_event(NmtEvent::AllCnsIdentified, &mut od);
        assert_eq!(nmt.current_state(), NmtState::NmtPreOperational2);

        // NMT_MT4
        nmt.process_event(NmtEvent::ConfigurationCompleteCnsReady, &mut od);
        assert_eq!(nmt.current_state(), NmtState::NmtReadyToOperate);

        // NMT_MT5
        nmt.process_event(NmtEvent::AllMandatoryCnsOperational, &mut od);
        assert_eq!(nmt.current_state(), NmtState::NmtOperational);
        assert_eq!(od.read_u8(0x1F8C, 0), Some(NmtState::NmtOperational as u8));
    }

    #[test]
    fn test_mn_boot_to_basic_ethernet() {
        let mut od = get_test_mn_od();
        // Set bit 13 in NMT_StartUp_U32
        od.write(0x1F80, 0, ObjectValue::Unsigned32(1 << 13))
            .unwrap();
        let mut nmt = MnNmtStateMachine::from_od(&od).unwrap();

        nmt.current_state = NmtState::NmtNotActive;

        // NMT_MT7
        nmt.process_event(NmtEvent::Timeout, &mut od);
        assert_eq!(nmt.current_state(), NmtState::NmtBasicEthernet);
        assert_eq!(
            od.read_u8(0x1F8C, 0),
            Some(NmtState::NmtBasicEthernet as u8)
        );
    }

    #[test]
    fn test_mn_error_handling_transition() {
        let mut od = get_test_mn_od();
        let mut nmt = MnNmtStateMachine::from_od(&od).unwrap();
        nmt.current_state = NmtState::NmtOperational;

        // NMT_MT6
        nmt.process_event(NmtEvent::Error, &mut od);
        assert_eq!(nmt.current_state(), NmtState::NmtPreOperational1);
        assert_eq!(
            od.read_u8(0x1F8C, 0),
            Some(NmtState::NmtPreOperational1 as u8)
        );
    }
}