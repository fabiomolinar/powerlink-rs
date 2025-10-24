// crates/powerlink-rs/src/nmt/events.rs

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
    /// Corresponds to the `NMTReset` command (hardware or other external reset).
    Reset,
    /// Corresponds to the `NMTSwReset` command.
    SwReset,
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
    /// Triggered when node received a SoC or SoA frame.
    SocSoAReceived,

    // --- Controlled Node (CN) Specific Events ---
    /// The CN received a SoC frame.
    SocReceived,
    /// Configuration completed and the CN is ready to operate.
    CnConfigurationComplete,
    /// Any powerlink frame received (for boot-up sequence).
    PowerlinkFrameReceived,

    // --- Managing Node (MN) Specific Events ---
    /// All mandatory CNs identified.
    AllCnsIdentified,
    /// MN configuration complete and all CNs ready to operate.
    ConfigurationCompleteCnsReady,
}
