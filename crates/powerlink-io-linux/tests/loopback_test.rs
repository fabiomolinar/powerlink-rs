#![cfg(target_os = "linux")]

use powerlink_io_linux::LinuxPnetInterface;
use powerlink_rs::{
    common::{NetTime, RelativeTime},
    frame::{PowerlinkFrame, SocFrame, SoAFrame, RequestedServiceId, ServiceId},
    nmt::flags::FeatureFlags,
    od::{AccessType, Category, Object, ObjectDictionary, ObjectEntry, ObjectValue, PdoMapping},
    types::{NodeId, C_ADR_MN_DEF_NODE_ID, EPLVersion},
    Codec, ControlledNode, NetworkInterface, Node, NodeAction,
};
use pnet::datalink::interfaces;
use std::{env, thread, time::{Duration, Instant}};

/// Helper function to find a suitable network interface for testing.
/// In Docker, this will be "eth0". On a host, it will be the loopback.
fn find_test_interface() -> String {
    // Inside a Docker container, the primary interface is typically eth0.
    // For local testing, we fall back to the loopback interface.
    interfaces()
        .into_iter()
        .find(|iface| iface.name == "eth0" && !iface.is_loopback())
        .map(|iface| iface.name)
        .unwrap_or_else(|| {
            interfaces()
                .into_iter()
                .find(|iface| iface.is_loopback())
                .map(|iface| iface.name)
                .expect("No suitable test interface (eth0 or loopback) found.")
        })
}

/// Helper function to create a minimal but valid Object Dictionary for a CN.
/// The ControlledNode constructor validates that these mandatory objects exist.
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
                ObjectValue::Boolean(0),
            ]),
            name: "NMT_EPLNodeID_REC",
            category: Category::Mandatory,
            access: None,
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
            object: Object::Variable(ObjectValue::Unsigned32(5_000_000)),
            name: "NMT_CNBasicEthernetTimeout_U32",
            category: Category::Mandatory,
            access: Some(AccessType::ReadWriteStore),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );
    od.insert(
        0x1F8C,
        ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned8(0)),
            name: "NMT_CurrNMTState_U8",
            category: Category::Mandatory,
            access: Some(AccessType::ReadOnly),
            default_value: None,
            value_range: None,
            pdo_mapping: Some(PdoMapping::No),
        },
    );
     od.insert(
        0x1F83,
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

    // Add object 0x1F98 for the IdentResponse payload builder
    od.insert(
        0x1F98,
        ObjectEntry {
            object: Object::Record(vec![
                ObjectValue::Unsigned32(0), // IsochrTxMaxPayload
                ObjectValue::Unsigned32(0), // IsochrRxMaxPayload
                ObjectValue::Unsigned32(0), // PResMaxLatency
                ObjectValue::Unsigned16(0), // PReqActPayloadLimit
                ObjectValue::Unsigned16(0), // PResActPayloadLimit
                ObjectValue::Unsigned32(0), // ASndMaxLatency
                ObjectValue::Unsigned8(0),  // MultiplCycleCnt
                ObjectValue::Unsigned16(1500), // AsyncMTU
            ]),
            name: "NMT_CycleTiming_REC",
            category: Category::Mandatory,
            access: None,
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );


    od
}


#[test]
// This test is now intended to be run inside Docker, which handles permissions.
// The #[ignore] can be removed if you primarily run tests via Docker.
#[ignore]
fn test_cn_responds_to_ident_request() {
    // This test now runs in one of two modes, determined by an environment variable.
    // This allows the same test binary to act as the MN or CN in different containers.
    let role = env::var("POWERLINK_TEST_ROLE").unwrap_or_else(|_| "MN".to_string());
    let test_interface = find_test_interface();

    if role == "CN" {
        run_cn_logic(&test_interface);
    } else {
        run_mn_logic(&test_interface);
    }
}

/// The logic for the Controlled Node container.
fn run_cn_logic(interface_name: &str) {
    let cn_node_id = 42;
    println!("[CN] Thread started on interface '{}'.", interface_name);
    let mut cn_interface =
        LinuxPnetInterface::new(interface_name, cn_node_id).unwrap();
    // The CN needs to know its own MAC to filter out sent packets on loopback
    let cn_mac = cn_interface.local_mac_address();

    let od = get_test_od(cn_node_id);
    let mut node = ControlledNode::new(od, cn_mac.into()).unwrap();
    println!("[CN] Initial NMT state: {:?}", node.nmt_state());

    let mut buffer = [0u8; 1518];
    let start_time = Instant::now();

    // Run for a limited time, waiting for the MN's frames.
    while start_time.elapsed() < Duration::from_secs(5) {
        match cn_interface.receive_frame(&mut buffer) {
            Ok(bytes) if bytes > 0 => {
                // Ignore frames sent by this node itself (loopback echo)
                let source_mac = &buffer[6..12];
                if source_mac == cn_mac {
                    continue;
                }
                
                println!("[CN] Received {} bytes.", bytes);
                if let NodeAction::SendFrame(response) =
                    node.process_raw_frame(&buffer[..bytes])
                {
                    println!("[CN] Sending IdentResponse of {} bytes.", response.len());
                    cn_interface.send_frame(&response).unwrap();
                    println!("[CN] Response sent. Thread finished.");
                    return; // The job is done, exit.
                }
                println!("[CN] New NMT state: {:?}", node.nmt_state());
            }
            _ => { // Timeout or error, just continue
            }
        }
    }
    println!("[CN] Thread timed out without sending a response.");
    panic!("[CN] Did not receive a valid SoA(IdentRequest) to respond to.");
}

/// The logic for the Managing Node container, which drives the test.
fn run_mn_logic(interface_name: &str) {
    let cn_node_id = 42;
    println!("[MN] Starting test driver on interface '{}'.", interface_name);

    // Give the CN container a moment to start up.
    thread::sleep(Duration::from_millis(500));

    let mut mn_interface =
        LinuxPnetInterface::new(interface_name, C_ADR_MN_DEF_NODE_ID).unwrap();
    let mn_mac = mn_interface.local_mac_address();

    // 1. Send SoC to move CN from NotActive -> PreOperational1
    let net_time = NetTime { seconds: 0, nanoseconds: 0 };
    let relative_time = RelativeTime { seconds: 0, nanoseconds: 0 };
    let soc_frame = SocFrame::new(mn_mac.into(), Default::default(), net_time, relative_time);
    let mut soc_buffer = vec![0u8; 64];
    let soc_size = soc_frame.serialize(&mut soc_buffer).unwrap();
    soc_buffer.truncate(soc_size);
    println!("[MN] Sending SoC...");
    mn_interface.send_frame(&soc_buffer).unwrap();
    thread::sleep(Duration::from_millis(100));

    // 2. Send SoA(IdentRequest) to the CN
    let soa = SoAFrame::new(
        mn_mac.into(),
        powerlink_rs::nmt::states::NmtState::NmtPreOperational1,
        Default::default(),
        RequestedServiceId::IdentRequest,
        NodeId(cn_node_id),
        EPLVersion(0),
    );
    let mut soa_buffer = vec![0u8; 64];
    let soa_size = soa.serialize(&mut soa_buffer).unwrap();
    soa_buffer.truncate(soa_size);
    println!("[MN] Sending SoA(IdentRequest)...");
    mn_interface.send_frame(&soa_buffer).unwrap();

    // 3. Receive the ASnd(IdentResponse) from the CN
    let mut receive_buffer = [0u8; 1518];
    println!("[MN] Waiting for IdentResponse...");
    let start_time = Instant::now();
    while start_time.elapsed() < Duration::from_secs(5) {
        if let Ok(bytes) = mn_interface.receive_frame(&mut receive_buffer) {
             // Ignore frames sent by this node itself (loopback echo)
            let source_mac = &receive_buffer[6..12];
            if source_mac == mn_mac {
                continue;
            }

            if bytes > 0 {
                println!("[MN] Received {} bytes.", bytes);
                if let Ok(PowerlinkFrame::ASnd(asnd)) =
                    powerlink_rs::deserialize_frame(&receive_buffer[..bytes])
                {
                    if asnd.source == NodeId(cn_node_id)
                        && asnd.service_id == ServiceId::IdentResponse
                    {
                        println!("[MN] Success! Received valid IdentResponse.");
                        // The test passes here.
                        return;
                    }
                }
            }
        }
    }

    panic!("Did not receive a valid ASnd(IdentResponse) frame from the CN.");
}

