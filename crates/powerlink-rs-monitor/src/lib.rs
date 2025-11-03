// crates/powerlink-rs-monitor/src/lib.rs

// This crate will require std for the web server and tokio
extern crate alloc;

// Module for the core data models
pub mod model;
// Module for the web server and WebSocket logic
mod server;

use log::{error, info};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

// Imports for 'in-process' mode
#[cfg(feature = "in-process")]
use crate::model::DiagnosticSnapshot;
#[cfg(feature = "in-process")]
use crossbeam_channel::Receiver;
#[cfg(feature = "in-process")]
use tokio::sync::broadcast;

// Imports for 'standalone' mode
#[cfg(feature = "standalone")]
use powerlink_rs::{ControlledNode, NetworkInterface, od::ObjectDictionary, types::NodeId};
#[cfg(feature = "standalone")]
use std::sync::Arc;

/// The default port for the web monitor.
const DEFAULT_MONITOR_PORT: u16 = 3000;
/// The capacity of the broadcast channel for WebSocket clients.
const BROADCAST_CHANNEL_CAPACITY: usize = 32;

/// Starts the web monitor in "in-process" mode.
///
/// This function is intended to be run in a dedicated, non-real-time thread
/// (e.g., by the main application thread after spawning the RT node thread).
/// It will start a web server and WebSocket endpoint.
///
/// * `receiver`: The `crossbeam-channel` to receive `DiagnosticSnapshot` updates from
///   the real-time POWERLINK node thread.
#[cfg(feature = "in-process")]
pub async fn start_in_process_monitor(
    receiver: Receiver<DiagnosticSnapshot>,
) -> Result<(), Box<dyn std::error::Error>> {
    
    // 1. Define the web server address.
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), DEFAULT_MONITOR_PORT);

    // 2. Create the tokio broadcast channel.
    // This channel will distribute snapshots from the bridge task to all
    // connected WebSocket clients.
    let (snapshot_tx, _) =
        broadcast::channel::<DiagnosticSnapshot>(BROADCAST_CHANNEL_CAPACITY);
    
    // 3. Spawn the blocking task to bridge the channels.
    // This is the most critical part. We use `spawn_blocking` to move
    // the blocking `receiver.recv()` call off of the async runtime,
    // preventing it from stalling the web server.
    let bridge_tx = snapshot_tx.clone();
    tokio::task::spawn_blocking(move || {
        info!("Starting RT-to-NRT channel bridge task.");
        // This loop will block on the crossbeam receiver
        while let Ok(snapshot) = receiver.recv() {
            // When a snapshot is received from the RT thread,
            // send it to the async broadcast channel.
            if let Err(e) = bridge_tx.send(snapshot) {
                // This typically means all WebSocket clients (and the server)
                // have disconnected.
                error!("Failed to broadcast snapshot (no receivers?): {}. Shutting down bridge.", e);
                break;
            }
        }
        info!("RT-to-NRT channel bridge task shut down.");
    });

    // 4. Start the web server.
    // This will run indefinitely, serving the root page and WebSocket connections.
    server::start_web_server(addr, snapshot_tx).await;
    
    Ok(())
}

/// Starts the web monitor in "standalone" (Out-of-Process) mode.
///
/// This function runs a full POWERLINK stack as a Diagnostic Node (CN 253),
/// using SDOs to poll the Managing Node (MN 240) for data.
///
/// * `interface`: A boxed, thread-safe `NetworkInterface` (e.g., `LinuxPnetInterface`)
///   provided by the application.
#[cfg(feature = "standalone")]
pub async fn start_standalone_monitor(
    mut interface: Box<dyn NetworkInterface + Send>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Standalone monitor (Node 253) logic would run here.");

    // 1. Create the OD for the diagnostic node
    let mut od = ObjectDictionary::new(None);
    // ... (populate OD with mandatory objects for a CN, Node ID 253) ...
    
    // 2. Create the ControlledNode instance
    // let mut node = ControlledNode::new(od, interface.local_mac_address().into())?;

    // 3. Spawn the web server (e.g., axum) in a separate async task
    
    // 4. Run the node's main loop in this task
    // loop {
    //     ... call node.tick() and node.process_raw_frame() ...
    //     ... periodically queue SDO reads to MN (0x1F8E, 0x1101, etc.) ...
    //     ... receive SDO responses and update web server state ...
    // }

    Ok(())
}