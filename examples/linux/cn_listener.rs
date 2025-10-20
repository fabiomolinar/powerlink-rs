use powerlink_io_linux::LinuxRawSocket;
use powerlink_rs::{ControlledNode, Node, NodeAction, ObjectDictionary};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // IMPORTANT: This needs to run with sudo!
    // And replace "eth0" with your actual network interface name.
    let mut interface = LinuxRawSocket::new("eth0", 42)?;

    // Create a basic OD for the node to initialize
    let od = ObjectDictionary::new(None);
    // In a real app, you would populate the OD with mandatory objects here.
    // For now, the `ControlledNode::new` might fail if validation is strict.
    // You may need to temporarily relax OD validation or populate it properly.
    
    let mut node = ControlledNode::new(od, interface.local_mac_address().into())?;

    println!("Listening for POWERLINK frames on interface 'eth0'...");

    let mut buffer = [0u8; 1518];
    loop {
        if let Ok(bytes_received) = interface.receive_frame(&mut buffer) {
            let action = node.process_raw_frame(&buffer[..bytes_received]);

            if let NodeAction::SendFrame(response_vec) = action {
                println!("Node generated a response. Sending frame...");
                interface.send_frame(&response_vec)?;
            }
        }
    }
}