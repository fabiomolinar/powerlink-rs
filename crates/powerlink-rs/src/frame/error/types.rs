#![allow(non_camel_case_types)]
use crate::types::NodeId;

/// An action for the NMT layer to take in response to a critical DLL error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NmtAction {
    None,
    ResetCommunication,
    ResetNode(NodeId),
}

/// Represents all possible DLL error symptoms and sources.
/// (EPS_DS_301, Section 4.7)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DllError {
    E_DLL_LOSS_OF_LINK, E_DLL_BAD_PHYS_MODE, E_DLL_MAC_BUFFER, E_DLL_CRC_TH, E_DLL_COLLISION_TH,
    E_DLL_INVALID_FORMAT, E_DLL_LOSS_SOC_TH, E_DLL_LOSS_SOA_TH, E_DLL_LOSS_PREQ_TH,
    E_DLL_LOSS_PRES_TH { node_id: NodeId }, E_DLL_LOSS_STATUSRES_TH { node_id: NodeId },
    E_DLL_CYCLE_EXCEED_TH, E_DLL_CYCLE_EXCEED, E_DLL_LATE_PRES_TH { node_id: NodeId },
    E_DLL_JITTER_TH, E_DLL_MS_WAIT_SOC, E_DLL_COLLISION, E_DLL_MULTIPLE_MN,
    E_DLL_ADDRESS_CONFLICT, E_DLL_MEV_ASND_TIMEOUT,
}