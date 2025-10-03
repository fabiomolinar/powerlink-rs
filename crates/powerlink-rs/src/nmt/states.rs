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