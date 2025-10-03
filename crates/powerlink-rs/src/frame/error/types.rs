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
    /// Corresponds to `E_DLL_CRC_TH` in the specification.
    CrcThreshold,
    /// Corresponds to `E_DLL_COLLISION_TH` in the specification.
    CollisionThreshold,
    /// Corresponds to `E_DLL_INVALID_FORMAT` in the specification.
    InvalidFormat,
    /// Corresponds to `E_DLL_LOSS_SOC_TH` in the specification.
    LossOfSocThreshold,
    /// Corresponds to `E_DLL_LOSS_SOA_TH` in the specification.
    LossOfSoaThreshold,
    /// Corresponds to `E_DLL_LOSS_PREQ_TH` in the specification.
    LossOfPreqThreshold,
    /// Corresponds to `E_DLL_LOSS_PRES_TH` in the specification.
    LossOfPresThreshold { node_id: NodeId },
    /// Corresponds to `E_DLL_LOSS_STATUSRES_TH` in the specification.
    LossOfStatusResThreshold { node_id: NodeId },
    /// Corresponds to `E_DLL_CYCLE_EXCEED_TH` in the specification.
    CycleExceededThreshold,
    /// Corresponds to `E_DLL_CYCLE_EXCEED` in the specification.
    CycleExceeded,
    /// Corresponds to `E_DLL_LATE_PRES_TH` in the specification.
    LatePresThreshold { node_id: NodeId },
    /// Corresponds to `E_DLL_JITTER_TH` in the specification.
    JitterThreshold,
    /// Corresponds to `E_DLL_MS_WAIT_SOC` in the specification.
    MsWaitSoc,
    /// Corresponds to `E_DLL_COLLISION` in the specification.
    Collision,
    /// Corresponds to `E_DLL_MULTIPLE_MN` in the specification.
    MultipleMn,
    /// Corresponds to `E_DLL_ADDRESS_CONFLICT` in the specification.
    AddressConflict,
    /// Corresponds to `E_DLL_MEV_ASND_TIMEOUT` in the specification.
    MevAsndTimeout,
}