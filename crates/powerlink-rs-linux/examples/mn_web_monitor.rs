// crates/powerlink-rs-linux/examples/mn_web_monitor.rs
//! This example application runs a POWERLINK Managing Node (MN)
//! and a real-time web monitor in parallel.
//!
//! It demonstrates the 'in-process' monitoring mode, where the node
//! and the web server run in the same application but in different
//! threads, communicating via an RT-safe channel.

use crossbeam_channel::{self, Sender};
use log::{error, info, trace, warn};
use powerlink_rs::{
    nmt::NmtStateMachine,
    node::{
        mn::{MnContext, CnState, ManagingNode},
        Node, NodeAction,
    },
    od::{AccessType, Category, Object, ObjectDictionary, ObjectEntry, ObjectValue},
    types::{C_ADR_MN_DEF_NODE_ID, NodeId},
    NetworkInterface, // Added this
};
use powerlink_rs_linux::LinuxPnetInterface;
use powerlink_rs_monitor::{
    model::{DiagnosticCounters, DiagnosticSnapshot, MnDllErrorCounters},
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

/// Helper function to create the network interface.
/// This will enable pcap logging if the PCAP_FILE_PATH env var is set.
fn create_interface(
    interface_name: &str,
    node_id: u8,
) -> Result<LinuxPnetInterface, String> {
    // Check if the "pcap" feature is enabled at compile time
    #[cfg(feature = "pcap")]
    {
        // If it is, check the environment variable at runtime
        if let Ok(pcap_path) = env::var("PCAP_FILE_PATH") {
            info!("[RT-Thread] PCAP logging enabled, writing to {}", pcap_path);
            return LinuxPnetInterface::with_pcap(interface_name, node_id, &pcap_path);
        }
    }

    // Default: create interface without pcap
    info!("[RT-Thread] PCAP logging disabled.");
    LinuxPnetInterface::new(interface_name, node_id)
}

/// This function runs the real-time POWERLINK node loop.
/// It must never block on non-RT tasks.
fn run_realtime_node(snapshot_tx: Sender<DiagnosticSnapshot>) -> Result<(), String> {
    // --- 1. Setup Interface ---
    let interface_name = env::var("POWERLINK_INTERFACE").unwrap_or_else(|_| "eth0".to_string());
    info!("[RT-Thread] Using interface: {}", interface_name);

    // Use our new helper function to create the interface
    let mut interface = create_interface(&interface_name, C_ADR_MN_DEF_NODE_ID)
        .map_err(|e| format!("Failed to create interface: {}", e))?;

    // Set a short read timeout so the loop can spin
    interface
        .set_read_timeout(Duration::from_millis(1))
        .map_err(|e| format!("Failed to set timeout: {:?}", e))?;

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

        // Receive frames
        let buffer_slice = match interface.receive_frame(&mut buffer) {
            Ok(bytes) if bytes > 0 => &buffer[..bytes],
            _ => &[], // Pass an empty slice on error or timeout
        };

        // Run the node's full cycle
        // This single call handles frame processing, ticking,
        // and action prioritization.
        let action = node.run_cycle(buffer_slice, current_time_us);

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
        let snapshot = build_snapshot(&node.context);
        let _ = snapshot_tx.try_send(snapshot); // Ignore error if channel is full

        // In a real application, we would sleep until `node.next_tick_us()`
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
        .map(|(id, info)| powerlink_rs_monitor::model::CnInfo {
            node_id: id.0,
            nmt_state: cn_state_to_string(info.state),
            communication_ok: info.communication_ok,
        })
        .collect();

    // --- FIX: Manually create the serializable DTO ---
    let core_counters = &context.dll_error_manager.counters;
    let monitor_counters = MnDllErrorCounters {
        crc_errors: core_counters.crc_errors.cumulative_count(),
        collision: core_counters.collision.cumulative_count(),
        cycle_time_exceeded: core_counters.cycle_time_exceeded.cumulative_count(),
        loss_of_link_cumulative: core_counters.loss_of_link_cumulative,
    };

    // 2. Build the snapshot
    DiagnosticSnapshot {
        // --- FIX: Call NmtStateMachine trait method (trait is in scope) ---
        mn_nmt_state: format!("{:?}", context.nmt_state_machine.current_state()),
        cn_states,
        dll_error_counters: monitor_counters, // <-- Use the new DTO
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
fn create_mn_od() -> ObjectDictionary<'static> {
    let mut od = ObjectDictionary::new(None);
    
    od.insert(
        0x1006, // NMT_CycleLen_U32
        ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned32(20000)), // 20ms
            name: "NMT_CycleLen_U32",
            category: Category::Mandatory,
            access: Some(AccessType::ReadWriteStore),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );
    od.insert(
        0x1101, // DIA_NMTTelegrCount_REC
        ObjectEntry {
            object: Object::Record(vec![
                ObjectValue::Unsigned32(0), // 1: IsochrCyc_U32
                ObjectValue::Unsigned32(0), // 2: IsochrRx_U32
                ObjectValue::Unsigned32(0), // 3: IsochrTx_U32
                ObjectValue::Unsigned32(0), // 4: AsyncRx_U32
                ObjectValue::Unsigned32(0), // 5: AsyncTx_U32
            ]),
            name: "DIA_NMTTelegrCount_REC",
            category: Category::Optional,
            access: None,
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );
    od.insert(
        0x1102, // DIA_ERRStatistics_REC
        ObjectEntry {
            object: Object::Record(vec![
                ObjectValue::Unsigned32(0), // 1: HistoryEntryWrite_U32
                ObjectValue::Unsigned32(0), // 2: EmergencyQueueOverflow_U32
            ]),
            name: "DIA_ERRStatistics_REC",
            category: Category::Optional,
            access: None,
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );

    let node_id = NodeId(42);
    od.insert(
        0x1F84, // NMT_MNNodeList_AU32 (Device Type List)
        ObjectEntry {
            // --- FIX: An Array holds ObjectValues, not Objects ---
            object: Object::Array(
                (0..=255)
                    .map(|_| ObjectValue::Unsigned32(0))
                    .collect(),
            ),
            name: "NMT_MNNodeList_AU32",
            category: Category::Optional,
            access: None,
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );
    od.insert(
        0x1F82, // NMT_MNNodeAssgmt_AU32 (Feature flags per node)
        ObjectEntry {
            // --- FIX: An Array holds ObjectValues, not Objects ---
            object: Object::Array(
                (0..=255)
                    .map(|_| ObjectValue::Unsigned32(0))
                    .collect(),
            ),
            name: "NMT_MNNodeAssgmt_AU32",
            category: Category::Mandatory,
            access: None,
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );

    // --- FIX: Use public .write() and pass an ObjectValue ---
    od.write(
        0x1F82,
        node_id.0.into(),
        ObjectValue::Unsigned32(0x0000_0004),
    )
    .unwrap(); // Mandatory CN
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