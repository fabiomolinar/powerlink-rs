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
    /// A consumer heartbeat timeout occurred for a monitored node. (Spec 7.3.5.1)
    HeartbeatTimeout { node_id: NodeId },
}

impl DllError {
    /// Maps a DllError to its corresponding error code from the specification.
    /// (Reference: EPSG DS 301, Appendix 3.9)
    pub fn to_error_code(&self) -> u16 {
        match self {
            DllError::LossOfLink => 0x8165,             // E_DLL_LOSS_OF_LINK
            DllError::BadPhysicalMode => 0x8161,        // E_DLL_BAD_PHYS_MODE
            DllError::MacBuffer => 0x8166,              // E_DLL_MAC_BUFFER
            DllError::Crc => 0x8164,                    // E_DLL_CRC_TH
            DllError::Collision => 0x8163,              // E_DLL_COLLISION_TH
            DllError::InvalidFormat => 0x8241,          // E_DLL_INVALID_FORMAT
            DllError::LossOfSoc => 0x8245,              // E_DLL_LOSS_SOC_TH
            DllError::LossOfSoa => 0x8244,              // E_DLL_LOSS_SOA_TH
            DllError::LossOfPreq => 0x8242,             // E_DLL_LOSS_PREQ_TH
            DllError::LossOfPres { .. } => 0x8243,      // E_DLL_LOSS_PRES_TH
            DllError::LossOfStatusRes { .. } => 0x8246, // E_DLL_LOSS_STATUSRES_TH
            DllError::CycleTimeExceeded => 0x8233,      // E_DLL_CYCLE_EXCEED_TH
            DllError::LatePres { .. } => 0x8236,        // E_DLL_LATE_PRES_TH
            DllError::SoCJitter => 0x8235,              // E_DLL_JITTER_TH
            DllError::PdoMapVersion { .. } => 0x8211,   // E_PDO_MAP_VERS
            DllError::PdoPayloadShort { .. } => 0x8210, // E_PDO_SHORT_RX
            // No specific code for heartbeat, use a custom one in the 82xx protocol range
            DllError::HeartbeatTimeout { .. } => 0x8250, // E_NMT_HEARTBEAT_TH (Custom)

            // The following are internal or MN-specific and don't have direct CN error codes.
            // A generic code could be used if necessary.
            _ => 0x8000, // Generic internal error, for logging purposes
        }
    }
}
