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
pub enum NmtState {
    // --- Generic States (GS) ---

    /// Corresponds to the `NMT_GS_OFF` state in the specification.
    NmtGsOff,
    /// A super-state. Corresponds to `NMT_GS_POWERED` in the specification.
    NmtGsPowered,
    /// A super-state for the initialisation process. Corresponds to `NMT_GS_INITIALISATION` in the specification.
    NmtGsInitialisation,
    /// Common initialisation state after power-on or reset. Corresponds to `NMT_GS_INITIALISING` in the specification.
    NmtGsInitialising,
    /// Resets the application-specific parts of the object dictionary. Corresponds to `NMT_GS_RESET_APPLICATION` in the specification.
    NmtGsResetApplication,
    /// Resets the communication-specific parts of the object dictionary. Corresponds to `NMT_GS_RESET_COMMUNICATION` in the specification.
    NmtGsResetCommunication,
    /// Resets the device configuration. Corresponds to `NMT_GS_RESET_CONFIGURATION` in the specification.
    NmtGsResetConfiguration,
    /// A super-state indicating the node is communicating. Corresponds to `NMT_GS_COMMUNICATING` in the specification.
    NmtGsCommunicating,

    // --- Controlled Node (CN) States (CS) ---

    /// The node is not part of the POWERLINK cycle. Corresponds to `NMT_CS_NOT_ACTIVE` in the specification.
    #[default]
    NmtCsNotActive,
    /// A super-state for POWERLINK operational modes. Corresponds to `NMT_CS_EPL_MODE` in the specification.
    NmtCsEplMode,
    /// The node can only perform SDO communication. Corresponds to `NMT_CS_PRE_OPERATIONAL_1` in the specification.
    NmtCsPreOperational1,
    /// The node participates in the isochronous cycle, but PDOs are invalid. Corresponds to `NMT_CS_PRE_OPERATIONAL_2` in the specification.
    NmtCsPreOperational2,
    /// The node signals readiness for operation to the MN. Corresponds to `NMT_CS_READY_TO_OPERATE` in the specification.
    NmtCsReadyToOperate,
    /// The node is fully operational, and PDO data is valid. Corresponds to `NMT_CS_OPERATIONAL` in the specification.
    NmtCsOperational,
    /// The node is in a controlled shutdown state. Corresponds to `NMT_CS_STOPPED` in the specification.
    NmtCsStopped,
    /// The node operates as a standard Ethernet device. Corresponds to `NMT_CS_BASIC_ETHERNET` in the specification.
    NmtCsBasicEthernet,

    // --- Managing Node (MN) States (MS) ---

    /// Corresponds to the `NMT_MS_NOT_ACTIVE` state in the specification.
    NmtMsNotActive,
    /// Corresponds to the `NMT_MS_EPL_MODE` state in the specification.
    NmtMsEplMode,
    /// Corresponds to the `NMT_MS_PRE_OPERATIONAL_1` state in the specification.
    NmtMsPreOperational1,
    /// Corresponds to the `NMT_MS_PRE_OPERATIONAL_2` state in the specification.
    NmtMsPreOperational2,
    /// Corresponds to the `NMT_MS_READY_TO_OPERATE` state in the specification.
    NmtMsReadyToOperate,
    /// Corresponds to the `NMT_MS_OPERATIONAL` state in the specification.
    NmtMsOperational,
    /// Corresponds to the `NMT_MS_BASIC_ETHERNET` state in the specification.
    NmtMsBasicEthernet,
}

impl NmtState {
    /// Parses a u8 into an NmtState with node-specific context.
    pub fn from_u8_with_context(value: u8, node_type: NodeType) -> Result<Self, PowerlinkError> {
        match value {
            0x1C => match node_type {
                NodeType::ControlledNode => Ok(NmtState::NmtCsNotActive),
                NodeType::ManagingNode => Ok(NmtState::NmtMsNotActive),
            },
            0x1D => match node_type {
                NodeType::ControlledNode => Ok(NmtState::NmtCsPreOperational1),
                NodeType::ManagingNode => Ok(NmtState::NmtMsPreOperational1),
            },
            0x5D => match node_type {
                NodeType::ControlledNode => Ok(NmtState::NmtCsPreOperational2),
                NodeType::ManagingNode => Ok(NmtState::NmtMsPreOperational2),
            },
            0x6D => match node_type {
                NodeType::ControlledNode => Ok(NmtState::NmtCsReadyToOperate),
                NodeType::ManagingNode => Ok(NmtState::NmtMsReadyToOperate),
            },
            0xFD => match node_type {
                NodeType::ControlledNode => Ok(NmtState::NmtCsOperational),
                NodeType::ManagingNode => Ok(NmtState::NmtMsOperational),
            },
            0x1E => match node_type {
                NodeType::ControlledNode => Ok(NmtState::NmtCsBasicEthernet),
                NodeType::ManagingNode => Ok(NmtState::NmtMsBasicEthernet),
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
            0x1C => Ok(NmtState::NmtCsNotActive),      // Also NmtMsNotActive
            0x1D => Ok(NmtState::NmtCsPreOperational1), // Also NmtMsPreOperational1
            0x5D => Ok(NmtState::NmtCsPreOperational2), // Also NmtMsPreOperational2
            0x6D => Ok(NmtState::NmtCsReadyToOperate),  // Also NmtMsReadyToOperate
            0xFD => Ok(NmtState::NmtCsOperational),     // Also NmtMsOperational
            0x4D => Ok(NmtState::NmtCsStopped),
            0x1E => Ok(NmtState::NmtCsBasicEthernet),   // Also NmtMsBasicEthernet
            _ => Err(PowerlinkError::InvalidFrame),
        }
    }
}