// examples/linux/cn_listener.rs
use powerlink_rs_linux::LinuxPnetInterface;
use powerlink_rs::{
    od::utils::new_cn_default,
    types::{IpAddress, NodeId}, // Added IpAddress
    ControlledNode,
    NetworkInterface,
    Node,
    NodeAction,
    // ObjectDictionary, // No longer directly used here, new_cn_default returns it
};
use std::time::{Duration, Instant}; // Added Time
use std::thread; // Added thread
use log::{error, info}; // Added log

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // IMPORTANT: This needs to run with sudo!
    // And replace "eth0" with your actual network interface name.
    let node_id = 42;
    let mut interface = LinuxPnetInterface::new("eth0", node_id)?;

    // Set a small read timeout so the loop is non-blocking
    interface.set_read_timeout(Duration::from_millis(1))?;

    // Create a basic OD for the node to initialize
    // Use the default CN OD constructor for a valid configuration
    let od = new_cn_default(NodeId(node_id));

    let mut node = ControlledNode::new(od, interface.local_mac_address().into())?;

    info!("Starting CN listener for Node {} on interface 'eth0'...", node_id);

    let mut eth_buffer = [0u8; 1518];
    let mut udp_buffer = [0u8; 1500]; // Buffer for UDP datagrams
    let start_time = Instant::now();

    loop {
        let current_time_us = start_time.elapsed().as_micros() as u64;

        // 1. Poll for Ethernet frames
        let eth_slice = match interface.receive_frame(&mut eth_buffer) {
            Ok(bytes) if bytes > 0 => Some(&eth_buffer[..bytes]),
            _ => None, // Pass None on error or timeout
        };

        // 2. Poll for UDP datagrams
        let udp_info: Option<(&[u8], IpAddress, u16)> = match interface.receive_udp(&mut udp_buffer) {
            Ok(Some((size, ip, port))) => Some((&udp_buffer[..size], ip, port)),
            _ => None, // Pass None on error or no data
        };

        // 3. Run the node's main cycle
        // This unified function handles frame processing, UDP processing, and internal ticks.
        let action = node.run_cycle(eth_slice, udp_info, current_time_us);

        // 4. Execute any returned action
        match action {
            NodeAction::SendFrame(response_vec) => {
                info!("Sending Ethernet frame ({} bytes)...", response_vec.len());
                if let Err(e) = interface.send_frame(&response_vec) {
                    error!("Failed to send Ethernet frame: {:?}", e);
                }
            }
            NodeAction::SendUdp {
                dest_ip,
                dest_port,
                data,
            } => {
                info!(
                    "Sending UDP frame ({} bytes) to {}:{}...",
                    data.len(),
                    core::net::Ipv4Addr::from(dest_ip),
                    dest_port
                );
                if let Err(e) = interface.send_udp(dest_ip, dest_port, &data) {
                    error!("Failed to send UDP frame: {:?}", e);
                }
            }
            NodeAction::NoAction => {
                // Nothing to do
            }
        }

        // In a real app, we would sleep until `node.next_action_time()`
        // For this simple listener, a tiny sleep is fine to yield CPU.
        thread::sleep(Duration::from_micros(100));
    }
}