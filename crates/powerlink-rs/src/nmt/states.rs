//+ NEW FILE
/// Defines the NMT states for a POWERLINK node, covering both the common
/// initialisation states and the specific CN states.
/// (EPSG DS 301, Section 7.1 and Appendix 3.6)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NMTState {
    /// Common initialisation state after power-on or reset.
    Initialising,
    /// Resets the application-specific parts of the object dictionary.
    ResetApplication,
    /// Resets the communication-specific parts of the object dictionary.
    ResetCommunication,
    /// Resets the device configuration using the current object dictionary.
    ResetConfiguration,
    
    /// CN state: The node is not yet part of the POWERLINK cycle.
    #[default]
    NotActive,
    /// CN state: The node can only perform SDO communication.
    PreOperational1,
    /// CN state: The node participates in the isochronous cycle, but PDOs are invalid.
    PreOperational2,
    /// CN state: The node signals readiness for operation to the MN.
    ReadyToOperate,
    /// CN state: The node is fully operational, and PDO data is valid.
    Operational,
    /// CN state: The node is in a controlled shutdown state and does not participate in PDO exchange.
    Stopped,
    /// CN state: The node operates as a standard Ethernet device.
    BasicEthernet,
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