use powerlink_rs_linux::LinuxPnetInterface;
use powerlink_rs::{
    od::utils::new_cn_default,
    types::NodeId,
    ControlledNode,
    NetworkInterface,
    Node,
    NodeAction,
    ObjectDictionary,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // IMPORTANT: This needs to run with sudo!
    // And replace "eth0" with your actual network interface name.
    let node_id = 42;
    let mut interface = LinuxPnetInterface::new("eth0", node_id)?;

    // Create a basic OD for the node to initialize
    // Use the default CN OD constructor for a valid configuration
    let od = new_cn_default(NodeId(node_id));
    
    let mut node = ControlledNode::new(od, interface.local_mac_address().into())?;

    println!("Listening for POWERLINK frames on interface 'eth0'...");

    let mut buffer = [0u8; 1518];
    loop {
        if let Ok(bytes_received) = interface.receive_frame(&mut buffer) {
            if bytes_received > 0 {
                // In a real app, you would get the current time.
                let current_time_us = 0; 
                let action = node.process_raw_frame(&buffer[..bytes_received], current_time_us);

                if let NodeAction::SendFrame(response_vec) = action {
                    println!("Node generated a response. Sending frame...");
                    interface.send_frame(&response_vec)?;
                }
            } else {
                // Timeout, call tick
                let current_time_us = 0;
                let _action = node.tick(current_time_us);
                // Handle tick action if needed
            }
        }
    }
}