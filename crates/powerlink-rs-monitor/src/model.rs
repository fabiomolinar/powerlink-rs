//! Defines the core data structures for diagnostic monitoring.
//!
//! These structs are used to pass data from the real-time node thread
//! to the non-real-time monitor thread and are serialized (e.g., to JSON)
//! for the web frontend.

use alloc::string::String;
use alloc::vec::Vec;
use serde::Serialize;
use serde_json::Value; // Keep this for arbitrary error counters

/// A serializable snapshot of a single Controlled Node's state,
/// as seen by the Managing Node.
#[derive(Serialize, Clone, Debug)]
pub struct CnInfo {
    pub node_id: u8,
    pub nmt_state: String,
    pub communication_ok: bool,
}

/// A serializable snapshot of the node's diagnostic counters,
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

/// The main data packet sent from the POWERLINK node to the monitor.
/// This contains a complete snapshot of the network's state for a given cycle.
#[derive(Serialize, Clone, Debug)]
pub struct DiagnosticSnapshot {
    /// The NMT state of the Managing Node.
    pub mn_nmt_state: String,
    /// A list of all known Controlled Nodes and their current states.
    pub cn_states: Vec<CnInfo>,
    /// A JSON-compatible representation of the node's generic DLL error counters.
    pub dll_error_counters: Value,
    /// A structured representation of the node's diagnostic counters (OD 0x1101/0x1102).
    pub diagnostic_counters: DiagnosticCounters,
}