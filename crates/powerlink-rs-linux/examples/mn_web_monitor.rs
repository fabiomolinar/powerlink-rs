//! This example application runs a POWERLINK Managing Node (MN)
//! and a real-time web monitor in parallel.
//!
//! It demonstrates the 'in-process' monitoring mode, where the node
//! and the web server run in the same application but in different
//! threads, communicating via an RT-safe channel.

use crossbeam_channel::{self, Sender};
use log::{debug, error, info, trace, warn};
use powerlink_rs::{
    frame::PowerlinkFrame,
    node::{
        mn::{
            state::{CnInfo, CnState, MnContext},
            ManagingNode,
        },
        Node, NodeAction,
    },
    od::{Object, ObjectDictionary, ObjectEntry, ObjectValue},
    types::{C_ADR_MN_DEF_NODE_ID, NodeId},
};
use powerlink_rs_linux::LinuxPnetInterface;
use powerlink_rs_monitor::{
    model::{DiagnosticCounters, DiagnosticSnapshot},
    start_in_process_monitor,
};
use std::{
    env, process, thread,
    time::{Duration, Instant},
};

/// The main entry point.
/// This function starts the `tokio` async runtime for the web server
/// and spawns a dedicated OS thread for the real-time POWERLINK node.
#[tokio::main]
async fn main() {
    env_logger::try_init().ok();

    // 1. Create the bounded, RT-safe channel.
    // A capacity of 1 ensures the RT thread never blocks, it just
    // drops a snapshot if the web server is too slow.
    let (snapshot_tx, snapshot_rx) = crossbeam_channel::bounded(1);

    // 2. Spawn the real-time node thread.
    // This thread will run the POWERLINK stack at a high priority
    // and send data *into* the snapshot_tx.
    thread::spawn(move || {
        if let Err(e) = run_realtime_node(snapshot_tx) {
            error!("[RT-Thread] Node failed: {}", e);
            process::exit(1);
        }
    });

    // 3. Run the non-real-time web monitor in the main thread.
    // This async function will run the web server and block forever,
    // receiving data *from* the snapshot_rx.
    info!("[NRT-Thread] Starting web monitor server...");
    if let Err(e) = start_in_process_monitor(snapshot_rx).await {
        error!("[NRT-Thread] Web monitor failed: {}", e);
        process::exit(1);
    }
}

/// This function runs the real-time POWERLINK node loop.
/// It must never block on non-RT tasks.
fn run_realtime_node(snapshot_tx: Sender<DiagnosticSnapshot>) -> Result<(), String> {
    // --- 1. Setup Interface ---
    let interface_name = env::var("POWERLINK_INTERFACE").unwrap_or_else(|_| "eth0".to_string());
    info!("[RT-Thread] Using interface: {}", interface_name);

    let mut interface = LinuxPnetInterface::new(&interface_name, C_ADR_MN_DEF_NODE_ID)
        .map_err(|e| format!("Failed to create interface: {}", e))?;
    
    // Set a short read timeout so the loop can spin
    interface.set_read_timeout(Duration::from_millis(1)).map_err(|e| format!("Failed to set timeout: {:?}", e))?;

    // --- 2. Setup Object Dictionary ---
    info!("[RT-Thread] Creating Object Dictionary...");
    let od = create_mn_od();

    // --- 3. Create Node ---
    let mut node = ManagingNode::new(od, interface.local_mac_address().into())
        .map_err(|e| format!("Failed to create ManagingNode: {:?}", e))?;

    // --- 4. Run Real-Time Loop ---
    let mut buffer = [0u8; 1518];
    let start_time = Instant::now();
    info!("[RT-Thread] Starting real-time node loop...");

    loop {
        let current_time_us = start_time.elapsed().as_micros() as u64;

        // 4a. Receive frames
        let action = match interface.receive_frame(&mut buffer) {
            Ok(bytes) if bytes > 0 => {
                let received_slice = &buffer[..bytes];
                if let Ok(frame) = PowerlinkFrame::deserialize(received_slice) {
                    node.process_powerlink_frame(frame, current_time_us)
                } else {
                    NodeAction::NoAction
                }
            }
            Ok(_) => {
                // No frame received, just tick the node
                node.tick(current_time_us)
            }
            Err(e) => {
                warn!("[RT-Thread] Receive error: {:?}", e);
                NodeAction::NoAction
            }
        };

        // 4b. Execute node actions
        match action {
            NodeAction::SendFrame(frame) => {
                trace!("[RT-Thread] Sending frame ({} bytes)", frame.len());
                if let Err(e) = interface.send_frame(&frame) {
                    error!("[RT-Thread] Send error: {:?}", e);
                }
            }
            NodeAction::NoAction => {
                // Nothing to do
            }
            _ => {
                // SDO or UDP actions (not handled in this simple example)
            }
        }

        // 4c. Send snapshot to monitor (non-blocking)
        // We build and send a snapshot on every loop. The bounded(1)
        // channel automatically handles throttling: if the web server
        // hasn't processed the last snapshot, this one is just dropped.
        let snapshot = build_snapshot(&node.context);
        let _ = snapshot_tx.try_send(snapshot); // Ignore error if channel is full

        // In a real application, we would sleep until `node.next_tick_us()`
        // For this example, a small spin-wait is fine.
        thread::sleep(Duration::from_micros(100));
    }
}

/// Creates a `DiagnosticSnapshot` from the node's internal context.
/// This is the "transformation" step.
fn build_snapshot(context: &MnContext) -> DiagnosticSnapshot {
    // 1. Transform internal CnInfo into serializable monitor::model::CnInfo
    let cn_states = context
        .node_info
        .iter()
        .map(|(id, info)| {
            powerlink_rs_monitor::model::CnInfo {
                node_id: id.0,
                nmt_state: cn_state_to_string(info.state),
                communication_ok: info.communication_ok,
            }
        })
        .collect();

    // 2. Build the snapshot
    DiagnosticSnapshot {
        mn_nmt_state: format!("{:?}", context.nmt_state_machine.current_state()),
        cn_states,
        // Use serde_json::to_value to convert arbitrary structs to JSON
        dll_error_counters: serde_json::to_value(&context.dll_error_manager.counters)
            .unwrap_or_default(),
        diagnostic_counters: build_diag_counters(&context.core.od),
    }
}

/// Helper to build the diagnostic counters struct from the OD.
fn build_diag_counters(od: &ObjectDictionary) -> DiagnosticCounters {
    DiagnosticCounters {
        isochr_cycles: od.read_u32(0x1101, 1).unwrap_or(0),
        isochr_rx: od.read_u32(0x1101, 2).unwrap_or(0),
        isochr_tx: od.read_u32(0x1101, 3).unwrap_or(0),
        async_rx: od.read_u32(0x1101, 4).unwrap_or(0),
        async_tx: od.read_u32(0x1101, 5).unwrap_or(0),
        emergency_queue_overflow: od.read_u32(0x1102, 2).unwrap_or(0),
    }
}

/// Helper to create a minimal Object Dictionary for the MN.
/// (Adapted from io_module_resources/mn_main.rs)
fn create_mn_od() -> ObjectDictionary<'static> {
    let mut od = ObjectDictionary::new(None);
    // Add mandatory objects (simplified)
    od.insert(
        0x1006, // NMT_CycleLen_U32
        ObjectEntry::variable(0x1006, "NMT_CycleLen_U32", ObjectValue::Unsigned32(20000)), // 20ms
    );
    // Add diagnostic counters
    od.insert(
        0x1101, // DIA_NMTTelegrCount_REC
        ObjectEntry::record(0x1101, "DIA_NMTTelegrCount_REC", vec![
            ObjectValue::Unsigned32(0), // 1: IsochrCyc_U32
            ObjectValue::Unsigned32(0), // 2: IsochrRx_U32
            ObjectValue::Unsigned32(0), // 3: IsochrTx_U32
            ObjectValue::Unsigned32(0), // 4: AsyncRx_U32
            ObjectValue::Unsigned32(0), // 5: AsyncTx_U32
        ])
    );
    od.insert(
        0x1102, // DIA_ERRStatistics_REC
        ObjectEntry::record(0x1102, "DIA_ERRStatistics_REC", vec![
            ObjectValue::Unsigned32(0), // 1: HistoryEntryWrite_U32
            ObjectValue::Unsigned32(0), // 2: EmergencyQueueOverflow_U32
        ])
    );
    // Add expected CN info (Node 42)
    let node_id = NodeId(42);
    od.insert(
        0x1F84, // NMT_MNNodeList_AU32 (Device Type List)
        ObjectEntry::array(0x1F84, "NMT_MNNodeList_AU32", 255, Object::Variable(ObjectValue::Unsigned32(0)))
    );
    od.insert(
        0x1F82, // NMT_MNNodeAssgmt_AU32 (Feature flags per node)
        ObjectEntry::array(0x1F82, "NMT_MNNodeAssgmt_AU32", 255, Object::Variable(ObjectValue::Unsigned32(0)))
    );
    od.write_u32(0x1F82, node_id.0, 0x0000_0004).unwrap(); // Mandatory CN
    od
}

/// Helper to convert internal CnState to a human-readable string.
fn cn_state_to_string(state: CnState) -> String {
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