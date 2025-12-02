// src/nmt/cn_state_machine.rs

use super::flags::FeatureFlags;
use super::state_machine::NmtStateMachine;
use super::states::NmtState;
use crate::PowerlinkError;
use crate::frame::DllError;
use crate::log::Loggable;
use crate::nmt::events::NmtEvent;
use crate::od::{ObjectDictionary, ObjectValue};
use crate::types::NodeId;
use alloc::vec::Vec;
use log::{debug, info};
use crate::log::{pl_debug, pl_info, pl_trace, pl_warn};
use alloc::string::String;
use alloc::format;

/// Manages the NMT state for a Controlled Node.
pub struct CnNmtStateMachine {
    pub current_state: NmtState,
    pub node_id: NodeId,
    pub feature_flags: FeatureFlags,
    pub basic_ethernet_timeout: u32,
    /// Latch to ensure we only enter ReadyToOperate if enabled by MN
    pub ready_to_operate_enabled: bool,
    /// Latch to ensure the Application has finished its configuration.
    pub app_ready: bool,
}

impl CnNmtStateMachine {
    pub fn new(node_id: NodeId, feature_flags: FeatureFlags, basic_ethernet_timeout: u32) -> Self {
        Self {
            current_state: NmtState::NmtGsInitialising,
            node_id,
            feature_flags,
            basic_ethernet_timeout,
            ready_to_operate_enabled: false,
            app_ready: true, 
        }
    }

    pub fn from_od(od: &ObjectDictionary) -> Result<Self, PowerlinkError> {
        let node_id_val = od.read(0x1F93, 1).ok_or(PowerlinkError::ObjectNotFound)?;
        let node_id = if let ObjectValue::Unsigned8(val) = &*node_id_val {
            NodeId::try_from(*val)?
        } else {
            return Err(PowerlinkError::TypeMismatch);
        };
        
        debug!("[CN - Node {}] Initializing CN NMT state machine from Object Dictionary.", node_id);

        let feature_flags_val = od.read(0x1F82, 0).ok_or(PowerlinkError::ObjectNotFound)?;
        let feature_flags = if let ObjectValue::Unsigned32(val) = &*feature_flags_val {
            FeatureFlags::from_bits_truncate(*val)
        } else {
            return Err(PowerlinkError::TypeMismatch);
        };

        let basic_ethernet_timeout_val =
            od.read(0x1F99, 0).ok_or(PowerlinkError::ObjectNotFound)?;
        let basic_ethernet_timeout =
            if let ObjectValue::Unsigned32(val) = &*basic_ethernet_timeout_val {
                *val
            } else {
                return Err(PowerlinkError::TypeMismatch);
            };

        info!(
            "[CN - Node {}] CN NMT configured with NodeId: {}, FeatureFlags: {:?}, BasicEthTimeout: {}",
            node_id, node_id, feature_flags, basic_ethernet_timeout
        );

        Ok(Self::new(node_id, feature_flags, basic_ethernet_timeout))
    }

    /// Updates the Object Dictionary with the current NMT state.
    /// This is critical because frame builders read the state from OD 0x1F8C.
    fn update_od_state(&self, od: &mut ObjectDictionary) {
        // NMT_CurrState_U8 (0x1F8C)
        let val = self.current_state as u8;
        // [FIX] Use write_internal to bypass RO access check.
        // 0x1F8C is defined as RO in the spec, so standard write() fails.
        if let Err(e) = od.write_internal(0x1F8C, 0, ObjectValue::Unsigned8(val), false) {
            pl_warn!(*self, "Failed to update NMT state in OD (0x1F8C): {:?}", e);
        } else {
            pl_trace!(*self, "Updated OD 0x1F8C to state {:?}", self.current_state);
        }
    }
}

impl NmtStateMachine for CnNmtStateMachine {
    fn node_id(&self) -> NodeId {
        self.node_id
    }

    fn is_cn(&self) -> bool {
        true
    }

    fn current_state(&self) -> NmtState {
        self.current_state
    }

    fn set_state(&mut self, new_state: NmtState) {
        self.current_state = new_state;
        if new_state != NmtState::NmtPreOperational2 && new_state != NmtState::NmtReadyToOperate {
            self.ready_to_operate_enabled = false;
        }
    }

    fn process_event(
        &mut self,
        event: NmtEvent,
        od: &mut ObjectDictionary,
    ) -> Option<Vec<DllError>> {
        let mut errors: Vec<DllError> = Vec::new();
        let old_state = self.current_state;

        if matches!(
            event,
            NmtEvent::Reset
                | NmtEvent::SwReset
                | NmtEvent::ResetNode
                | NmtEvent::ResetCommunication
                | NmtEvent::ResetConfiguration
        ) {
            self.reset(event, od);
            // Ensure OD is updated after reset
            self.update_od_state(od);
            return None;
        }

        pl_trace!(*self, 
            "[NMT] Processing event {:?} in state {:?}",
            event, old_state
        );
        let next_state = match (self.current_state, event) {
            // --- CN Boot-up Sequence ---

            // (NMT_CT2) A SoC or SoA frame moves the node from NotActive to PreOp1.
            (NmtState::NmtNotActive, NmtEvent::SocReceived | NmtEvent::SoAReceived) => {
                NmtState::NmtPreOperational1
            }
            // (NMT_CT3) A timeout in NotActive leads to BasicEthernet mode.
            (NmtState::NmtNotActive, NmtEvent::Timeout) => NmtState::NmtBasicEthernet,

            // (NMT_CT4) Receiving a SoC in PreOp1 signals the start of the isochronous phase.
            (NmtState::NmtPreOperational1, NmtEvent::SocReceived) => NmtState::NmtPreOperational2,

            // (NMT_CT5) The MN enables the next state.
            (NmtState::NmtPreOperational2, NmtEvent::EnableReadyToOperate) => {
                pl_debug!(*self, "Received EnableReadyToOperate.");
                self.ready_to_operate_enabled = true;
                
                // If the application is already ready, we transition immediately.
                if self.app_ready {
                    pl_info!(*self, "Transition to ReadyToOperate (App was already ready).");
                    NmtState::NmtReadyToOperate
                } else {
                    pl_debug!(*self, "Waiting for App Configuration.");
                    NmtState::NmtPreOperational2
                }
            }
            
            // [Fix] Handle StartNode in PreOp2 for robustness.
            // Some MN implementations might skip EnableReadyToOperate or it might be missed.
            (NmtState::NmtPreOperational2, NmtEvent::StartNode) => {
                pl_warn!(*self, "Received StartNode in PreOp2. Treating as implicit EnableReadyToOperate + StartNode.");
                self.ready_to_operate_enabled = true;
                if self.app_ready {
                    pl_info!(*self, "Transitioning directly to Operational (Implicit ReadyToOperate).");
                    NmtState::NmtOperational
                } else {
                    pl_debug!(*self, "StartNode received, but App not ready. Waiting.");
                    NmtState::NmtPreOperational2
                }
            }

            (NmtState::NmtReadyToOperate, NmtEvent::EnableReadyToOperate) => {
                pl_debug!(*self, "Ignored redundant EnableReadyToOperate command (already in state).");
                NmtState::NmtReadyToOperate
            }

            // (NMT_CT6) The application signals it's ready. 
            (NmtState::NmtPreOperational2, NmtEvent::CnConfigurationComplete) => {
                pl_debug!(*self, "Application signaled ConfigurationComplete.");
                self.app_ready = true;
                if self.ready_to_operate_enabled {
                    pl_info!(*self, "Transition to ReadyToOperate (MN was already enabled).");
                    NmtState::NmtReadyToOperate
                } else {
                    pl_debug!(*self, "App Config complete, but waiting for MN EnableReadyToOperate.");
                    NmtState::NmtPreOperational2
                }
            }
            
            // Allow re-signaling in ReadyToOp (idempotent)
            (NmtState::NmtReadyToOperate, NmtEvent::CnConfigurationComplete) => {
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
                // Log unexpected event only if it's not a common noise event
                // Only treat it as an error if it's not one of the events we explicitly ignore above
                pl_warn!(*self, "Unexpected Event {:?} in State {:?}", event, current);
                errors.push(DllError::UnexpectedEventInState {
                    state: current as u8,
                    event: event as u8,
                });
                current
            }
        };

        if old_state != next_state {
            pl_info!(*self, 
                "[NMT] State changed from {:?} to {:?}",
                old_state, next_state
            );
            self.set_state(next_state); 
            self.update_od_state(od);
        }

        if errors.is_empty() {
            None
        } else {
            Some(errors)
        }
    }
}

impl Loggable for CnNmtStateMachine {
    fn log_prefix(&self) -> String {
        format!("[CN - Node {}]", self.node_id)
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

        nmt.process_event(NmtEvent::SocReceived, &mut od);
        assert_eq!(nmt.current_state(), NmtState::NmtPreOperational1);

        nmt.process_event(NmtEvent::SocReceived, &mut od);
        assert_eq!(nmt.current_state(), NmtState::NmtPreOperational2);

        nmt.process_event(NmtEvent::EnableReadyToOperate, &mut od);
        assert_eq!(nmt.current_state(), NmtState::NmtReadyToOperate);

        nmt.process_event(NmtEvent::CnConfigurationComplete, &mut od);
        assert_eq!(nmt.current_state(), NmtState::NmtReadyToOperate);

        nmt.process_event(NmtEvent::StartNode, &mut od);
        assert_eq!(nmt.current_state(), NmtState::NmtOperational);
        assert_eq!(od.read_u8(0x1F8C, 0), Some(NmtState::NmtOperational as u8));
    }

    #[test]
    fn test_boot_up_robustness_start_node_in_preop2() {
        let mut od = get_test_od();
        let mut nmt = get_test_nmt();
        nmt.current_state = NmtState::NmtPreOperational2;
        nmt.app_ready = true;

        // Directly receiving StartNode in PreOp2 should work now
        nmt.process_event(NmtEvent::StartNode, &mut od);
        assert_eq!(nmt.current_state(), NmtState::NmtOperational);
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