// crates/powerlink-rs/src/nmt/cn_state_machine.rs

use super::flags::FeatureFlags;
use super::state_machine::NmtStateMachine;
use super::states::NmtState;
use crate::PowerlinkError;
use crate::frame::DllError;
use crate::nmt::events::NmtEvent;
use crate::od::{ObjectDictionary, ObjectValue};
use crate::types::NodeId;
use alloc::vec::Vec;
use log::{debug, info, trace};

/// Manages the NMT state for a Controlled Node.
pub struct CnNmtStateMachine {
    pub current_state: NmtState,
    pub node_id: NodeId,
    pub feature_flags: FeatureFlags,
    pub basic_ethernet_timeout: u32,
}

impl CnNmtStateMachine {
    /// Creates a new NMT state machine with pre-validated parameters.
    pub fn new(node_id: NodeId, feature_flags: FeatureFlags, basic_ethernet_timeout: u32) -> Self {
        Self {
            current_state: NmtState::NmtGsInitialising,
            node_id,
            feature_flags,
            basic_ethernet_timeout,
        }
    }

    /// A fallible constructor that reads its configuration from an Object Dictionary.
    pub fn from_od(od: &ObjectDictionary) -> Result<Self, PowerlinkError> {
        debug!("Initializing CN NMT state machine from Object Dictionary.");
        // Read Node ID from OD entry 0x1F93, sub-index 1.
        let node_id_val = od.read(0x1F93, 1).ok_or(PowerlinkError::ObjectNotFound)?;
        let node_id = if let ObjectValue::Unsigned8(val) = &*node_id_val {
            NodeId::try_from(*val)?
        } else {
            return Err(PowerlinkError::TypeMismatch);
        };

        // Read Feature Flags from OD entry 0x1F82, sub-index 0.
        let feature_flags_val = od.read(0x1F82, 0).ok_or(PowerlinkError::ObjectNotFound)?;
        let feature_flags = if let ObjectValue::Unsigned32(val) = &*feature_flags_val {
            FeatureFlags::from_bits_truncate(*val)
        } else {
            return Err(PowerlinkError::TypeMismatch);
        };

        // Read Basic Ethernet Timeout from OD entry 0x1F99, sub-index 0.
        let basic_ethernet_timeout_val =
            od.read(0x1F99, 0).ok_or(PowerlinkError::ObjectNotFound)?;
        let basic_ethernet_timeout =
            if let ObjectValue::Unsigned32(val) = &*basic_ethernet_timeout_val {
                *val
            } else {
                return Err(PowerlinkError::TypeMismatch);
            };

        info!(
            "CN NMT configured with NodeId: {}, FeatureFlags: {:?}, BasicEthTimeout: {}",
            node_id.0, feature_flags, basic_ethernet_timeout
        );

        Ok(Self::new(node_id, feature_flags, basic_ethernet_timeout))
    }
}

impl NmtStateMachine for CnNmtStateMachine {
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
    fn process_event(
        &mut self,
        event: NmtEvent,
        od: &mut ObjectDictionary,
    ) -> Option<Vec<DllError>> {
        let mut errors: Vec<DllError> = Vec::new();
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
            self.reset(event);
            if old_state != self.current_state {
                self.update_od_state(od);
            }
            // After a reset, a full re-initialisation sequence should run.
            self.run_internal_initialisation(od);
            return None;
        }

        trace!(
            "[NMT] Processing event {:?} in state {:?}",
            event, old_state
        );
        let next_state = match (self.current_state, event) {
            // --- CN Boot-up Sequence ---

            // (NMT_CT2) A SoC or SoA frame moves the node from NotActive to PreOp1.
            (NmtState::NmtNotActive, NmtEvent::SocReceived | NmtEvent::SocSoAReceived) => {
                NmtState::NmtPreOperational1
            }
            // (NMT_CT3) A timeout in NotActive leads to BasicEthernet mode.
            (NmtState::NmtNotActive, NmtEvent::Timeout) => NmtState::NmtBasicEthernet,

            // (NMT_CT4) Receiving a SoC in PreOp1 signals the start of the isochronous phase.
            (NmtState::NmtPreOperational1, NmtEvent::SocReceived) => NmtState::NmtPreOperational2,

            // (NMT_CT5) The MN enables the next state, but we wait for application readiness.
            (NmtState::NmtPreOperational2, NmtEvent::EnableReadyToOperate) => {
                debug!("Received EnableReadyToOperate, waiting for application confirmation.");
                NmtState::NmtPreOperational2
            }
            // (NMT_CT6) The application signals it's ready, moving to ReadyToOperate.
            (NmtState::NmtPreOperational2, NmtEvent::CnConfigurationComplete) => {
                NmtState::NmtReadyToOperate
            }
            // (NMT_CT7) The MN commands the CN to start full operation.
            (NmtState::NmtReadyToOperate, NmtEvent::StartNode) => NmtState::NmtOperational,

            // --- Operational State Transitions ---

            // (NMT_CT8) The MN can stop a node from several states.
            (
                NmtState::NmtPreOperational2
                | NmtState::NmtReadyToOperate
                | NmtState::NmtOperational,
                NmtEvent::StopNode,
            ) => NmtState::NmtCsStopped,

            // (NMT_CT9) The MN can command a node to return to PreOp2.
            (NmtState::NmtOperational, NmtEvent::EnterPreOperational2) => {
                NmtState::NmtPreOperational2
            }
            // (NMT_CT10) The MN can bring a stopped node back to PreOp2.
            (NmtState::NmtCsStopped, NmtEvent::EnterPreOperational2) => {
                NmtState::NmtPreOperational2
            }
            // (NMT_CT11) A critical error in any cyclic state forces a reset to PreOp1.
            (
                NmtState::NmtPreOperational2
                | NmtState::NmtReadyToOperate
                | NmtState::NmtOperational
                | NmtState::NmtCsStopped,
                NmtEvent::Error,
            ) => NmtState::NmtPreOperational1,

            // (NMT_CT12) Receiving a POWERLINK frame while in BasicEthernet forces a return to PreOp1.
            (NmtState::NmtBasicEthernet, NmtEvent::PowerlinkFrameReceived) => {
                NmtState::NmtPreOperational1
            }

            // If no specific transition is defined, remain in the current state.
            (current, _) => {
                errors.push(DllError::UnexpectedEventInState {
                    state: current as u8,
                    event: event as u8,
                });
                current
            }
        };

        if old_state != next_state {
            info!(
                "[NMT] State changed from {:?} to {:?}",
                old_state, next_state
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
    use crate::od::AccessType;
    use crate::od::{Object, ObjectEntry};
    use alloc::vec;

    fn get_test_od() -> ObjectDictionary<'static> {
        let mut od = ObjectDictionary::new(None);
        od.insert(
            0x1F93,
            ObjectEntry {
                object: Object::Record(vec![ObjectValue::Unsigned8(42), ObjectValue::Boolean(0)]),
                name: "NMT_EPLNodeID_REC",
                access: Some(AccessType::ReadWrite),
                default_value: None,
                value_range: None,
                pdo_mapping: None,
                category: crate::od::Category::Optional,
            },
        );
        let flags = FeatureFlags::ISOCHRONOUS | FeatureFlags::SDO_ASND | FeatureFlags::SDO_UDP;
        od.insert(
            0x1F82,
            ObjectEntry {
                object: Object::Variable(ObjectValue::Unsigned32(flags.0)),
                name: "NMT_FeatureFlags_U32",
                access: Some(AccessType::Constant),
                default_value: None,
                value_range: None,
                pdo_mapping: None,
                category: crate::od::Category::Optional,
            },
        );
        od.insert(
            0x1F99,
            ObjectEntry {
                object: Object::Variable(ObjectValue::Unsigned32(5_000_000)),
                name: "NMT_CNBasicEthernetTimeout_U32",
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

    // Helper for creating a state machine for tests
    fn get_test_nmt() -> CnNmtStateMachine {
        let node_id = NodeId::try_from(42).unwrap();
        let feature_flags = FeatureFlags::ISOCHRONOUS | FeatureFlags::SDO_ASND;
        CnNmtStateMachine::new(node_id, feature_flags, 5_000_000)
    }

    #[test]
    fn test_from_od_reads_parameters() {
        let od = get_test_od();
        let nmt = CnNmtStateMachine::from_od(&od).unwrap();
        assert_eq!(nmt.node_id, NodeId(42));
        assert!(nmt.feature_flags.contains(FeatureFlags::SDO_ASND));
        assert_eq!(nmt.basic_ethernet_timeout, 5_000_000);
    }

    #[test]
    fn test_from_od_fails_if_missing_objects() {
        let od = ObjectDictionary::new(None);
        let result = CnNmtStateMachine::from_od(&od);
        assert_eq!(result.err(), Some(PowerlinkError::ObjectNotFound));
    }

    #[test]
    fn test_internal_boot_sequence() {
        let mut od = get_test_od();
        let mut nmt = get_test_nmt();
        assert_eq!(nmt.current_state(), NmtState::NmtGsInitialising);
        nmt.run_internal_initialisation(&mut od);
        assert_eq!(nmt.current_state(), NmtState::NmtNotActive);
        assert_eq!(od.read_u8(0x1F8C, 0), Some(NmtState::NmtNotActive as u8));
    }

    #[test]
    fn test_full_boot_up_happy_path() {
        let mut od = get_test_od();
        let mut nmt = get_test_nmt();
        nmt.current_state = NmtState::NmtNotActive;

        nmt.process_event(NmtEvent::SocSoAReceived, &mut od);
        assert_eq!(nmt.current_state(), NmtState::NmtPreOperational1);

        nmt.process_event(NmtEvent::SocReceived, &mut od);
        assert_eq!(nmt.current_state(), NmtState::NmtPreOperational2);

        nmt.process_event(NmtEvent::EnableReadyToOperate, &mut od);
        assert_eq!(nmt.current_state(), NmtState::NmtPreOperational2);

        nmt.process_event(NmtEvent::CnConfigurationComplete, &mut od);
        assert_eq!(nmt.current_state(), NmtState::NmtReadyToOperate);

        nmt.process_event(NmtEvent::StartNode, &mut od);
        assert_eq!(nmt.current_state(), NmtState::NmtOperational);
        assert_eq!(od.read_u8(0x1F8C, 0), Some(NmtState::NmtOperational as u8));
    }

    #[test]
    fn test_error_handling_transition() {
        let mut od = get_test_od();
        let mut nmt = get_test_nmt();
        nmt.current_state = NmtState::NmtOperational;

        nmt.process_event(NmtEvent::Error, &mut od);
        assert_eq!(nmt.current_state(), NmtState::NmtPreOperational1);
    }

    #[test]
    fn test_stop_and_restart_node() {
        let mut od = get_test_od();
        let mut nmt = get_test_nmt();
        nmt.current_state = NmtState::NmtOperational;

        nmt.process_event(NmtEvent::StopNode, &mut od);
        assert_eq!(nmt.current_state(), NmtState::NmtCsStopped);

        nmt.process_event(NmtEvent::EnterPreOperational2, &mut od);
        assert_eq!(nmt.current_state(), NmtState::NmtPreOperational2);
    }
}
