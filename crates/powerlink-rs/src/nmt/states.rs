#![allow(non_camel_case_types)]
/// Defines the NMT states for a POWERLINK node, covering both the common
/// initialisation states and the specific CN states.
/// (EPSG DS 301, Section 7.1 and Appendix 3.6)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NMTState {
    // # MN and CN States
    NMT_GS_OFF,
    // # Powered
    NMT_GS_POWERED, // Super state
    // ## Initializations
    NMT_GS_INITIALISATION,  // Super state
    /// Common initialisation state after power-on or reset.
    NMT_GS_INITIALISING,
    /// Resets the application-specific parts of the object dictionary.
    NMT_GS_RESET_APPLICATION,
    /// Resets the communication-specific parts of the object dictionary.
    NMT_GS_RESET_COMMUNICATION,
    /// Resets the device configuration using the current object dictionary.
    NMT_GS_RESET_CONFIGURATION,
    // ## Communicating
    NMT_GS_COMMUNICATING, // Super state    
    /// CN state: The node is not yet part of the POWERLINK cycle.
    #[default]
    NMT_CS_NOT_ACTIVE,
    // ### EPL MODE
    NMT_CS_EPL_MODE, // Super state
    /// CN state: The node can only perform SDO communication.
    NMT_CS_PRE_OPERATIONAL_1,
    /// CN state: The node participates in the isochronous cycle, but PDOs are invalid.
    NMT_CS_PRE_OPERATIONAL_2,
    /// CN state: The node signals readiness for operation to the MN.
    NMT_CS_READY_TO_OPERATE,
    /// CN state: The node is fully operational, and PDO data is valid.
    NMT_CS_OPERATIONAL,
    /// CN state: The node is in a controlled shutdown state and does not participate in PDO exchange.
    NMT_CS_STOPPED,
    /// CN state: The node operates as a standard Ethernet device.
    NMT_CS_BASIC_ETHERNET,
    /// MN State
    NMT_MS_NOT_ACTIVE,
    NMT_MS_EPL_MODE,
    NMT_MS_PRE_OPERATIONAL_1,
    NMT_MS_PRE_OPERATIONAL_2,
    NMT_MS_READY_TO_OPERATE,
    NMT_MS_OPERATIONAL,
    NMT_MS_BASIC_ETHERNET
}

/// Defines events that can trigger a state transition in the NMT state machine.
/// These are derived from NMT commands or internal conditions.
/// (EPSG DS 301, Table 107)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NmtEvent {
    /// Corresponds to the NMTStartNode command.
    StartNode,
    /// Corresponds to the NMTStopNode command.
    StopNode,
    /// Corresponds to the NMTEnterPreOperational2 command.
    EnterPreOperational2,
    /// Corresponds to the NMTEnableReadyToOperate command.
    EnableReadyToOperate,
    /// Corresponds to the NMTResetNode command.
    ResetNode,
    /// Corresponds to the NMTResetCommunication command.
    ResetCommunication,
    /// Corresponds to the NMTResetConfiguration command.
    ResetConfiguration,
    /// Triggered internally or by receiving a POWERLINK frame in certain states.
    EnterEplMode,
    /// Triggered when a timer expires, e.g., waiting for frames in NotActive.
    Timeout,
    /// Triggered by a significant DLL or application error.
    Error,
}