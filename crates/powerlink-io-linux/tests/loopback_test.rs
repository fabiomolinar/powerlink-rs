#![cfg(target_os = "linux")]

use powerlink_io_linux::LinuxPnetInterface;
use powerlink_rs::{
    common::{NetTime, RelativeTime},
    frame::{PowerlinkFrame, SocFrame, SoAFrame, RequestedServiceId, ServiceId, ASndFrame},
    nmt::{flags::FeatureFlags, states::{NmtState}},
    od::{AccessType, Category, Object, ObjectDictionary, ObjectEntry, ObjectValue, PdoMapping},
    types::{NodeId, C_ADR_MN_DEF_NODE_ID, EPLVersion},
    Codec, ControlledNode, NetworkInterface, Node, NodeAction, PowerlinkError,
};
use pnet::datalink::interfaces;
use std::{env, thread, time::{Duration, Instant}};
use log::{debug, error, info}; // Make sure log macros are imported if used directly

// Define constants for clarity
const CN_NODE_ID: u8 = 42;
const TEST_INTERFACE: &str = "eth0"; // Standard interface name inside Docker
const FALLBACK_INTERFACE: &str = "lo"; // For local testing
const MAX_RECEIVE_ATTEMPTS: u32 = 40; // Approx 2 seconds with 50ms sleep
const SLEEP_DURATION: Duration = Duration::from_millis(50);

/// Helper function to find the network interface for testing.
/// Prefers "eth0" (common in Docker) but falls back to "lo" (loopback).
fn find_test_interface() -> String {
    interfaces()
        .into_iter()
        .find(|iface| iface.name == TEST_INTERFACE)
        .map(|iface| iface.name.clone())
        .unwrap_or_else(|| FALLBACK_INTERFACE.to_string())
}


/// Helper function to create a minimal but valid Object Dictionary for a CN.
fn get_test_od(node_id: u8) -> ObjectDictionary<'static> {
    let mut od = ObjectDictionary::new(None);

    // Mandatory objects for initialization, based on DS 301.
    od.insert(
        0x1000,
        ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned32(0)),
            name: "NMT_DeviceType_U32",
            category: Category::Mandatory,
            access: Some(AccessType::Constant),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );
    od.insert(
        0x1018,
        ObjectEntry {
            object: Object::Record(vec![
                ObjectValue::Unsigned32(0), // VendorId
                ObjectValue::Unsigned32(0), // ProductCode
                ObjectValue::Unsigned32(0), // RevisionNo
                ObjectValue::Unsigned32(0), // SerialNo
            ]),
            name: "NMT_IdentityObject_REC",
            category: Category::Mandatory,
            access: None,
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );
    od.insert(
        0x1F93,
        ObjectEntry {
            object: Object::Record(vec![
                ObjectValue::Unsigned8(node_id),
                ObjectValue::Boolean(0), // Node ID by HW = FALSE
            ]),
            name: "NMT_EPLNodeID_REC",
            category: Category::Mandatory,
            access: None, // Access type is for the whole object; sub-indices can be different
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );
    od.insert(
        0x1F82,
        ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned32(FeatureFlags::empty().0)),
            name: "NMT_FeatureFlags_U32",
            category: Category::Mandatory,
            access: Some(AccessType::Constant),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );
     od.insert(
        0x1F99,
        ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned32(5_000_000)), // 5 seconds
            name: "NMT_CNBasicEthernetTimeout_U32",
            category: Category::Mandatory,
            access: Some(AccessType::ReadWriteStore),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );
     od.insert(
        0x1F8C, // NMT_CurrNMTState_U8 - ReadOnly Mandatory Status Object
        ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned8(NmtState::NmtGsInitialising as u8)),
            name: "NMT_CurrNMTState_U8",
            category: Category::Mandatory,
            access: Some(AccessType::ReadOnly),
            default_value: None, // Initial state set by state machine
            value_range: None,
            pdo_mapping: Some(PdoMapping::No),
        },
    );
    od.insert(
        0x1F83, // NMT_EPLVersion_U8 - Constant Mandatory Version Object
        ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned8(0x15)), // V1.5
            name: "NMT_EPLVersion_U8",
            category: Category::Mandatory,
            access: Some(AccessType::Constant),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );
    // Add other necessary OD entries here as required by ControlledNode::new or the test logic.
    // Example: PDO mapping/communication parameters if testing PDOs.

    od
}


/// Logic for the Controlled Node (CN) role in the test.
fn run_cn_logic(interface_name: &str) {
    // Initialize the logger for this thread/process
    env_logger::try_init().ok(); // Ignore error if already initialized

    info!("[CN] Thread started. Interface: {}", interface_name);
    let mut cn_interface = match LinuxPnetInterface::new(interface_name, CN_NODE_ID) {
        Ok(iface) => iface,
        Err(e) => {
            error!("[CN] Failed to create interface: {}", e);
            panic!("[CN] Interface creation failed.");
        }
    };
    let od = get_test_od(CN_NODE_ID);
    let cn_mac = cn_interface.local_mac_address();
    let mut node = match ControlledNode::new(od, cn_mac.into()) {
         Ok(n) => n,
         Err(e) => {
             error!("[CN] Failed to create ControlledNode: {:?}", e);
             panic!("[CN] Node creation failed.");
         }
    };

    info!("[CN] Initial NMT state: {:?}", node.nmt_state());

    let mut buffer = [0u8; 1518]; // Standard MTU
    let start_time = Instant::now();
    let timeout = Duration::from_secs(10); // Adjust timeout as needed

    // Main receive loop for CN
    loop {
        // Check for timeout
        if start_time.elapsed() > timeout {
             error!("[CN] Test timed out waiting for expected frames.");
             panic!("[CN] Test timed out.");
        }

        match cn_interface.receive_frame(&mut buffer) {
            Ok(bytes) if bytes > 0 => {
                 debug!("[CN] Received {} bytes.", bytes);
                 let received_slice = &buffer[..bytes];

                 // Simple MAC filter: Ignore frames sent by self (loopback echo)
                 if bytes >= 12 && received_slice[6..12] == cn_mac {
                     debug!("[CN] Ignoring received frame from self.");
                     continue;
                 }

                // Process the frame using the node logic
                let action = node.process_raw_frame(received_slice);
                info!("[CN] New NMT state: {:?}", node.nmt_state());

                // Handle actions requested by the node (e.g., sending a response)
                if let NodeAction::SendFrame(response) = action {
                    info!("[CN] Sending response of {} bytes.", response.len());
                    if let Err(e) = cn_interface.send_frame(&response) {
                         error!("[CN] Failed to send response: {:?}", e);
                         // Decide how to handle send errors, maybe panic or retry
                    }
                     // If this response was the IdentResponse, the CN's job is done for this test
                     if let Ok(PowerlinkFrame::ASnd(ref asnd)) = powerlink_rs::deserialize_frame(&response) {
                         if asnd.service_id == ServiceId::IdentResponse {
                             info!("[CN] IdentResponse sent. Exiting CN logic.");
                             return; // Successfully completed the task
                         }
                     }
                }
            }
            Ok(_) => { /* No frame received, continue loop */ }
            Err(PowerlinkError::IoError) => { /* Expected on timeout in pnet */ }
            Err(e) => {
                error!("[CN] Receive error: {:?}", e);
                // Consider if non-timeout errors should cause panic
            }
        }
        // Small sleep to prevent busy-waiting
        thread::sleep(Duration::from_millis(10));
    }
}


/// Logic for the Managing Node (MN) role in the test.
fn run_mn_logic(interface_name: &str) {
    // Initialize the logger for this thread/process
    env_logger::try_init().ok(); // Ignore error if already initialized

    info!("[MN] Starting MN logic. Interface: {}", interface_name);
    let mut mn_interface =
        match LinuxPnetInterface::new(interface_name, C_ADR_MN_DEF_NODE_ID) {
             Ok(iface) => iface,
             Err(e) => {
                 error!("[MN] Failed to create interface: {}", e);
                 panic!("[MN] Interface creation failed.");
             }
        };
    let mn_mac = mn_interface.local_mac_address();

    // Give CN time to initialize (adjust if needed)
    thread::sleep(Duration::from_millis(500));

    // 1. Send SoC to move CN from NotActive -> PreOperational1
    // Use zero times for simplicity in this test.
    let net_time = NetTime { seconds: 0, nanoseconds: 0 };
    let relative_time = RelativeTime { seconds: 0, nanoseconds: 0 };
    let soc_frame = SocFrame::new(mn_mac.into(), Default::default(), net_time, relative_time);
    let mut soc_buffer = vec![0u8; 64]; // Ensure buffer is large enough
    let soc_size = soc_frame.serialize(&mut soc_buffer).unwrap();
    soc_buffer.truncate(soc_size);
    info!("[MN] Sending SoC...");
    mn_interface.send_frame(&soc_buffer).expect("Failed to send SoC");

    // Allow time for CN to process SoC
    thread::sleep(SLEEP_DURATION);

    // 2. Send SoA(IdentRequest) to the CN
    let soa = SoAFrame::new(
        mn_mac.into(),
        NmtState::NmtPreOperational1, // MN signals its current understanding of CN state
        Default::default(),
        RequestedServiceId::IdentRequest,
        NodeId(CN_NODE_ID),
        EPLVersion(0x15), // Example version V1.5
    );
    let mut soa_buffer = vec![0u8; 64]; // Ensure buffer is large enough
    let soa_size = soa.serialize(&mut soa_buffer).unwrap();
    soa_buffer.truncate(soa_size);
    info!("[MN] Sending SoA(IdentRequest)...");
    mn_interface.send_frame(&soa_buffer).expect("Failed to send SoA");

    // 3. Receive the ASnd(IdentResponse) from the CN
    let mut receive_buffer = [0u8; 1518];
    info!("[MN] Waiting for IdentResponse...");
    for i in 0..MAX_RECEIVE_ATTEMPTS {
        match mn_interface.receive_frame(&mut receive_buffer) {
            Ok(bytes) if bytes > 0 => {
                debug!("[MN] Received {} bytes on attempt {}.", bytes, i);
                let received_slice = &receive_buffer[..bytes];

                // Simple MAC filter: Ignore frames sent by self (loopback echo)
                if bytes >= 12 && received_slice[6..12] == mn_mac {
                    debug!("[MN] Ignoring received frame from self.");
                    continue;
                }

                // Check if it's the expected IdentResponse
                if let Ok(PowerlinkFrame::ASnd(asnd)) =
                    powerlink_rs::deserialize_frame(received_slice)
                {
                    if asnd.source == NodeId(CN_NODE_ID)
                        && asnd.destination == NodeId(C_ADR_MN_DEF_NODE_ID)
                        && asnd.service_id == ServiceId::IdentResponse
                    {
                        info!("[MN] Success! Received valid IdentResponse.");
                        // Optional: Add further checks on the payload content
                        return; // Test successful!
                    } else {
                         debug!("[MN] Received ASnd, but not the expected IdentResponse: {:?}", asnd);
                    }
                } else if let Ok(other_frame) = powerlink_rs::deserialize_frame(received_slice) {
                     debug!("[MN] Received other valid POWERLINK frame: {:?}", other_frame);
                } else {
                     debug!("[MN] Received invalid or non-POWERLINK frame.");
                }
            }
            Ok(_) => { /* No frame received, continue loop */ }
            Err(PowerlinkError::IoError) => { /* Expected on timeout in pnet */ }
            Err(e) => {
                 error!("[MN] Receive error: {:?}", e);
            }
        }
        thread::sleep(SLEEP_DURATION);
    }

    // If loop completes without success
    error!("[MN] Did not receive a valid ASnd(IdentResponse) frame from the CN.");
    panic!("Did not receive a valid ASnd(IdentResponse) frame from the CN.");
}


/// This test dispatches to either the MN or CN logic based on the
/// `POWERLINK_TEST_ROLE` environment variable. This allows the same test
/// binary to be used in both Docker containers.
#[test]
#[ignore] // Ignore this test when running locally, it's meant for Docker.
fn test_cn_responds_to_ident_request() {
     // Use TEST_INTERFACE in Docker, fallback to loopback for local runs
     let test_interface_name = find_test_interface();

    // Read the role from the environment variable. Default to "MN" if not set.
    let role = env::var("POWERLINK_TEST_ROLE").unwrap_or_else(|_| "MN".to_string());

    if role == "CN" {
        run_cn_logic(&test_interface_name);
    } else {
        // Run MN logic (potentially spawning CN logic if testing locally without Docker)
        // For Docker tests, this branch runs in the MN container,
        // while the CN container runs the run_cn_logic branch independently.
        run_mn_logic(&test_interface_name);
    }
}

