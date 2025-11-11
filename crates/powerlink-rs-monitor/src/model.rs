// crates/powerlink-rs-monitor/src/model.rs
//! Defines the core data structures for diagnostic monitoring.
//!
//! These structs are used to pass data from the real-time node thread
//! to the non-real-time monitor thread and are serialized (e.g., to JSON)
//! for the web frontend.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use powerlink_rs::nmt::NmtStateMachine;
use serde::Serialize;

// --- Imports from the powerlink-rs core crate ---
use powerlink_rs::{
    frame::error::MnErrorCounters as CoreMnErrorCounters,
    node::{CnState, mn::MnContext},
    od::ObjectDictionary,
};
// -------------------------------------------------

/// A serializable snapshot of a single Controlled Node's state,
/// as seen by the Managing Node.
#[derive(Serialize, Clone, Debug)]
pub struct CnInfo {
    pub node_id: u8,
    pub nmt_state: String,
    pub communication_ok: bool,
}

impl CnInfo {
    /// Helper to convert the internal `CnState` enum to a human-readable string.
    fn state_to_string(state: CnState) -> String {
        match state {
            CnState::Unknown => "Unknown",
            CnState::Identified => "Identified",
            CnState::PreOperational => "PreOperational",
            CnState::Operational => "Operational",
            CnState::Stopped => "Stopped",
            CnState::Missing => "Missing",
        }
        .to_string()
    }
}

/// A serializable DTO for the node's diagnostic counters,
/// primarily from OD 0x1101 and 0x1102.
#[derive(Serialize, Clone, Debug, Default)]
pub struct DiagnosticCounters {
    // NMT cycle counters (OD 0x1101)
    pub isochr_cycles: u32,
    pub isochr_rx: u32,
    pub isochr_tx: u32,
    pub async_rx: u32,
    pub async_tx: u32,

    // Error statistics (OD 0x1102)
    pub emergency_queue_overflow: u32,
}

impl DiagnosticCounters {
    /// Creates a `DiagnosticCounters` snapshot from the node's Object Dictionary.
    pub fn from_od(od: &ObjectDictionary) -> Self {
        Self {
            isochr_cycles: od.read_u32(0x1101, 1).unwrap_or(0),
            isochr_rx: od.read_u32(0x1101, 2).unwrap_or(0),
            isochr_tx: od.read_u32(0x1101, 3).unwrap_or(0),
            async_rx: od.read_u32(0x1101, 4).unwrap_or(0),
            async_tx: od.read_u32(0x1101, 5).unwrap_or(0),
            emergency_queue_overflow: od.read_u32(0x1102, 2).unwrap_or(0),
        }
    }
}

/// A serializable DTO for the node's internal DLL error counters.
/// This struct mirrors `powerlink_rs::frame::MnErrorCounters`
/// to provide a stable serialization API.
#[derive(Serialize, Clone, Debug, Default)]
pub struct MnDllErrorCounters {
    pub crc_errors: u32,
    pub collision: u32,
    pub cycle_time_exceeded: u32,
    pub loss_of_link_cumulative: u32,
}

impl MnDllErrorCounters {
    /// Creates a `MnDllErrorCounters` snapshot from the core `MnErrorCounters`.
    pub fn from_core(counters: &CoreMnErrorCounters) -> Self {
        Self {
            crc_errors: counters.crc_errors.cumulative_count(),
            collision: counters.collision.cumulative_count(),
            cycle_time_exceeded: counters.cycle_time_exceeded.cumulative_count(),
            loss_of_link_cumulative: counters.loss_of_link_cumulative,
        }
    }
}

/// The main data packet sent from the POWERLINK node to the monitor.
/// This contains a complete snapshot of the network's state for a given cycle.
#[derive(Serialize, Clone, Debug)]
pub struct DiagnosticSnapshot {
    /// The NMT state of the Managing Node.
    pub mn_nmt_state: String,
    /// A list of all known Controlled Nodes and their current states.
    pub cn_states: Vec<CnInfo>,
    /// A structured representation of the node's internal DLL error counters.
    pub dll_error_counters: MnDllErrorCounters,
    /// A structured representation of the node's diagnostic counters (OD 0x1101/0x1102).
    pub diagnostic_counters: DiagnosticCounters,
}

impl DiagnosticSnapshot {
    /// Creates a complete `DiagnosticSnapshot` from the node's `MnContext`.
    ///
    /// This is the primary function used by the application to build
    /// the data packet for the monitor.
    pub fn from_context(context: &MnContext) -> Self {
        // 1. Transform internal CnInfo into serializable monitor::model::CnInfo
        let cn_states = context
            .node_info
            .iter()
            .map(|(id, info)| CnInfo {
                node_id: id.0,
                nmt_state: CnInfo::state_to_string(info.state),
                communication_ok: info.communication_ok,
            })
            .collect();

        // 2. Build the snapshot
        DiagnosticSnapshot {
            mn_nmt_state: format!("{:?}", context.nmt_state_machine.current_state()),
            cn_states,
            dll_error_counters: MnDllErrorCounters::from_core(&context.dll_error_manager.counters),
            diagnostic_counters: DiagnosticCounters::from_od(&context.core.od),
        }
    }
}
