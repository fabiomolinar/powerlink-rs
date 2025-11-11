//! This example application runs a POWERLINK Managing Node (MN)
//! and a real-time web monitor in parallel.
//!
//! It demonstrates the 'in-process' monitoring mode, where the node
//! and the web server run in the same application but in different
//! threads, communicating via an RT-safe channel.
//!
//! This example can be ran with docker by running:
//! `docker-compose -f crates/powerlink-rs-linux/examples/mn_web_monitor_resources/docker-compose.yml up --build`

use crossbeam_channel::{self, Sender};
use log::{error, info, trace};
use powerlink_rs::{
    NetworkInterface,
    node::{Node, NodeAction, mn::ManagingNode},
    od::utils::new_mn_default,
    types::{C_ADR_MN_DEF_NODE_ID, IpAddress, NodeId}, // Import IpAddress
};
use powerlink_rs_linux::LinuxPnetInterface;
use powerlink_rs_monitor::{model::DiagnosticSnapshot, start_in_process_monitor};
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
fn create_interface(interface_name: &str, node_id: u8) -> Result<LinuxPnetInterface, String> {
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
    let od = new_mn_default(NodeId(C_ADR_MN_DEF_NODE_ID));

    // --- 3. Create Node ---
    let mut node = ManagingNode::new(od, interface.local_mac_address().into())
        .map_err(|e| format!("Failed to create ManagingNode: {:?}", e))?;

    // --- 4. Run Real-Time Loop ---
    let mut eth_buffer = [0u8; 1518];
    // This example is part of `powerlink-rs-linux`, which enables `sdo-udp`
    let mut udp_buffer = [0u8; 1500]; // Buffer for UDP datagrams
    let start_time = Instant::now();
    info!("[RT-Thread] Starting real-time node loop...");

    loop {
        let current_time_us = start_time.elapsed().as_micros() as u64;

        // 1. Poll for Ethernet frames
        let eth_slice = match interface.receive_frame(&mut eth_buffer) {
            Ok(bytes) if bytes > 0 => Some(&eth_buffer[..bytes]),
            _ => None, // Pass None on error or timeout
        };

        // 2. Poll for UDP datagrams
        let udp_info: Option<(&[u8], IpAddress, u16)> = match interface.receive_udp(&mut udp_buffer)
        {
            Ok(Some((size, ip, port))) => Some((&udp_buffer[..size], ip, port)),
            _ => None, // Pass None on error or no data
        };

        // 3. Run the node's full cycle with all available inputs
        // Since `powerlink-rs-linux` enables `sdo-udp`, we *must* use the 3-argument version.
        let action = node.run_cycle(eth_slice, udp_info, current_time_us);

        // 4. Execute node actions
        match action {
            NodeAction::SendFrame(frame) => {
                trace!("[RT-Thread] Sending frame ({} bytes)", frame.len());
                if let Err(e) = interface.send_frame(&frame) {
                    error!("[RT-Thread] Send error: {:?}", e);
                }
            }
            NodeAction::SendUdp {
                dest_ip,
                dest_port,
                data,
            } => {
                trace!(
                    "[RT-Thread] Sending UDP ({} bytes) to {}:{}",
                    data.len(),
                    core::net::Ipv4Addr::from(dest_ip),
                    dest_port
                );
                if let Err(e) = interface.send_udp(dest_ip, dest_port, &data) {
                    error!("[RT-Thread] UDP Send error: {:?}", e);
                }
            }
            NodeAction::NoAction => {
                // Nothing to do
            }
        }

        // 4c. Send snapshot to monitor (non-blocking)
        let snapshot = DiagnosticSnapshot::from_context(&node.context);
        let _ = snapshot_tx.try_send(snapshot); // Ignore error if channel is full

        // In a real application, we would sleep until `node.next_tick_us()`
        // For this example, a small sleep is fine to yield CPU.
        thread::sleep(Duration::from_micros(100));
    }
}
