use crate::types::NodeId;

/// An action for the NMT layer to take in response to a critical DLL error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NmtAction {
    /// No action is required.
    None,
    /// Reset the communication profile.
    ResetCommunication,
    /// Reset a specific node.
    ResetNode(NodeId),
}

/// Represents all possible DLL error symptoms and sources.
/// (Reference: EPSG DS 301, Section 4.7)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DllError {
    /// Corresponds to `E_DLL_LOSS_OF_LINK` in the specification.
    LossOfLink,
    /// Corresponds to `E_DLL_BAD_PHYS_MODE` in the specification.
    BadPhysicalMode,
    /// Corresponds to `E_DLL_MAC_BUFFER` in the specification.
    MacBuffer,
    /// Corresponds to `E_DLL_CRC` in the specification.
    Crc,
    /// Corresponds to `E_DLL_COLLISION` in the specification.
    Collision,
    /// Corresponds to `E_DLL_INVALID_FORMAT` in the specification.
    InvalidFormat,
    /// Corresponds to `E_DLL_LOSS_SOC` in the specification.
    LossOfSoc,
    /// Corresponds to `E_DLL_LOSS_SOA` in the specification.
    LossOfSoa,
    /// Corresponds to `E_DLL_LOSS_PREQ` in the specification.
    LossOfPreq,
    /// Corresponds to `E_DLL_LOSS_PRES` in the specification.
    LossOfPres { node_id: NodeId },
    /// Corresponds to `E_DLL_LOSS_STATUSRES` in the specification.
    LossOfStatusRes { node_id: NodeId },
    /// Corresponds to `E_DLL_CYCLE_EXCEED` in the specification.
    CycleTimeExceeded,
    /// Corresponds to `E_DLL_LATE_PRES` in the specification.
    LatePres { node_id: NodeId },
    /// Corresponds to `E_DLL_JITTER` in the specification.
    SoCJitter,
    /// Corresponds to `E_DLL_MS_WAIT_SOC` in the specification.
    MsWaitSoc,
    /// Corresponds to `E_DLL_MULTIPLE_MN` in the specification.
    MultipleMn,
    /// Corresponds to `E_DLL_ADDRESS_CONFLICT` in the specification.
    AddressConflict,
    /// Corresponds to `E_DLL_MEV_ASND_TIMEOUT` in the specification.
    MevAsndTimeout,
    /// Unexpected event in the current state machine state.
    UnexpectedEventInState { state: u8, event: u8 },
    /// Corresponds to `E_PDO_MAP_VERS` (Section 6.4.8.1.1)
    PdoMapVersion { node_id: NodeId },
    /// Corresponds to `E_PDO_SHORT_RX` (Section 6.4.8.1.2)
    PdoPayloadShort { node_id: NodeId },
}
