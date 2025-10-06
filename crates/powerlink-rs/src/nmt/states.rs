use crate::PowerlinkError;

/// Used to specify the node type for context-aware parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeType {
    ControlledNode,
    ManagingNode,
}

/// Defines the NMT states for a POWERLINK node.
///
/// This covers both the common initialisation states and the specific states
/// for Controlled Nodes (CN) and Managing Nodes (MN).
/// (Reference: EPSG DS 301, Section 7.1 and Appendix 3.6)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum NmtState {
    // NMT States. Super states are not coded.
    // --- Generic States (GS) ---

    /// Corresponds to the `NMT_GS_OFF` state in the specification.
    NmtGsOff = 0b0000_0000,
    /// Common initialisation state after power-on or reset. Corresponds to `NMT_GS_INITIALISING` in the specification.
    NmtGsInitialising = 0b0001_1001,
    /// Resets the application-specific parts of the object dictionary. Corresponds to `NMT_GS_RESET_APPLICATION` in the specification.
    NmtGsResetApplication = 0b0010_1001,
    /// Resets the communication-specific parts of the object dictionary. Corresponds to `NMT_GS_RESET_COMMUNICATION` in the specification.
    NmtGsResetCommunication = 0b0011_1001,
    /// Resets the device configuration. Corresponds to `NMT_GS_RESET_CONFIGURATION` in the specification.
    NmtGsResetConfiguration = 0b0111_1001,

    // --- Controlled Node (CN) States (CS) ---

    /// The node is not part of the POWERLINK cycle. Corresponds to `NMT_CS_NOT_ACTIVE` and `NMT_MS_NOT_ACTIVE` in the specification.
    #[default]
    NmtNotActive = 0b0001_1100,
    /// The node can only perform SDO communication. Corresponds to `NMT_CS_PRE_OPERATIONAL_1` and `NMT_MS_PRE_OPERATIONAL_1` in the specification.
    NmtPreOperational1 = 0b0001_1101,
    /// The node participates in the isochronous cycle, but PDOs are invalid. Corresponds to `NMT_CS_PRE_OPERATIONAL_2` and `NMT_MS_PRE_OPERATIONAL_2` in the specification.
    NmtPreOperational2 = 0b0101_1101,
    /// The node signals readiness for operation to the MN. Corresponds to `NMT_CS_READY_TO_OPERATE` and `NMT_MS_READY_TO_OPERATE` in the specification.
    NmtReadyToOperate = 0b0110_1101,
    /// The node is fully operational, and PDO data is valid. Corresponds to `NMT_CS_OPERATIONAL` and `NMT_MS_OPERATIONAL` in the specification.
    NmtOperational = 0b1111_1101,
    /// The node is in a controlled shutdown state. Corresponds to `NMT_CS_STOPPED` in the specification.
    NmtCsStopped = 0b0100_1101,
    /// The node operates as a standard Ethernet device. Corresponds to `NMT_CS_BASIC_ETHERNET` and `NMT_MS_BASIC_ETHERNET` in the specification.
    NmtBasicEthernet = 0b0001_1110,
}

impl NmtState {
    /// Parses a u8 into an NmtState with node-specific context.
    pub fn from_u8_with_context(value: u8, node_type: NodeType) -> Result<Self, PowerlinkError> {
        match value {
            0x1C => match node_type {
                NodeType::ControlledNode => Ok(NmtState::NmtNotActive),
                NodeType::ManagingNode => Ok(NmtState::NmtNotActive),
            },
            0x1D => match node_type {
                NodeType::ControlledNode => Ok(NmtState::NmtPreOperational1),
                NodeType::ManagingNode => Ok(NmtState::NmtPreOperational1),
            },
            0x5D => match node_type {
                NodeType::ControlledNode => Ok(NmtState::NmtPreOperational2),
                NodeType::ManagingNode => Ok(NmtState::NmtPreOperational2),
            },
            0x6D => match node_type {
                NodeType::ControlledNode => Ok(NmtState::NmtReadyToOperate),
                NodeType::ManagingNode => Ok(NmtState::NmtReadyToOperate),
            },
            0xFD => match node_type {
                NodeType::ControlledNode => Ok(NmtState::NmtOperational),
                NodeType::ManagingNode => Ok(NmtState::NmtOperational),
            },
            0x1E => match node_type {
                NodeType::ControlledNode => Ok(NmtState::NmtBasicEthernet),
                NodeType::ManagingNode => Ok(NmtState::NmtBasicEthernet),
            },
            // Unambiguous states can be handled by the standard TryFrom.
            _ => NmtState::try_from(value),
        }
    }
}

/// Defines events that can trigger a state transition in the NMT state machine.
///
/// These are derived from NMT commands or internal conditions.
/// (Reference: EPSG DS 301, Table 107)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NmtEvent {
    /// Corresponds to the `NMTStartNode` command.
    StartNode,
    /// Corresponds to the `NMTStopNode` command.
    StopNode,
    /// Corresponds to the `NMTEnterPreOperational2` command.
    EnterPreOperational2,
    /// Corresponds to the `NMTEnableReadyToOperate` command.
    EnableReadyToOperate,
    /// Corresponds to the `NMTResetNode` command.
    ResetNode,
    /// Corresponds to the `NMTResetCommunication` command.
    ResetCommunication,
    /// Corresponds to the `NMTResetConfiguration` command.
    ResetConfiguration,
    /// Triggered internally or by receiving a POWERLINK frame.
    EnterEplMode,
    /// Triggered when a timer expires.
    Timeout,
    /// Triggered by a significant DLL or application error.
    Error,
}

impl TryFrom<u8> for NmtState {
    type Error = PowerlinkError;

    /// Converts a raw u8 value into an NmtState enum variant.
    /// This implementation covers all unique state values from EPSG DS 301, Appendix 3.6.
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            // Note: Several logical states share the same numeric value.
            // For deserialization, we map to the most common/expected state.
            // The internal state machine logic will handle transitions correctly.
            0x00 => Ok(NmtState::NmtGsOff),
            0x19 => Ok(NmtState::NmtGsInitialising),
            0x29 => Ok(NmtState::NmtGsResetApplication),
            0x39 => Ok(NmtState::NmtGsResetCommunication),
            0x79 => Ok(NmtState::NmtGsResetConfiguration),
            0x1C => Ok(NmtState::NmtNotActive),      // Also NmtMsNotActive
            0x1D => Ok(NmtState::NmtPreOperational1), // Also NmtMsPreOperational1
            0x5D => Ok(NmtState::NmtPreOperational2), // Also NmtMsPreOperational2
            0x6D => Ok(NmtState::NmtReadyToOperate),  // Also NmtMsReadyToOperate
            0xFD => Ok(NmtState::NmtOperational),     // Also NmtMsOperational
            0x4D => Ok(NmtState::NmtCsStopped),
            0x1E => Ok(NmtState::NmtBasicEthernet),   // Also NmtMsBasicEthernet
            _ => Err(PowerlinkError::InvalidFrame),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_try_from_u8_unambiguous() {
        assert_eq!(NmtState::try_from(0x00), Ok(NmtState::NmtGsOff));
        assert_eq!(NmtState::try_from(0x4D), Ok(NmtState::NmtCsStopped));
        assert!(NmtState::try_from(0xFF).is_err());
    }

    #[test]
    fn test_as_u8() {
        assert_eq!(NmtState::NmtGsOff as u8, 0x00);
        assert_eq!(NmtState::NmtCsStopped as u8, 0x4D);
        assert_eq!(NmtState::NmtPreOperational1 as u8, 0x1D);        
    }

    #[test]
    fn test_from_u8_with_context() {
        // Test a shared value
        assert_eq!(
            NmtState::from_u8_with_context(0xFD, NodeType::ControlledNode),
            Ok(NmtState::NmtOperational)
        );
        assert_eq!(
            NmtState::from_u8_with_context(0xFD, NodeType::ManagingNode),
            Ok(NmtState::NmtOperational)
        );

        // Test a non-shared value (falls back to TryFrom)
        assert_eq!(
            NmtState::from_u8_with_context(0x4D, NodeType::ControlledNode),
            Ok(NmtState::NmtCsStopped)
        );
        assert_eq!(
            NmtState::from_u8_with_context(0x4D, NodeType::ManagingNode),
            Ok(NmtState::NmtCsStopped)
        );
    }
}