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
use std::sync::mpsc;
use std::{thread, time::Duration};

/// Helper function to find the loopback network interface by name.
fn find_loopback() -> Option<String> {
    interfaces()
        .into_iter()
        .find(|iface| iface.is_loopback())
        .map(|iface| iface.name)
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

    od
}

#[test]
#[ignore] // This test requires root privileges, so ignore it by default.
fn test_loopback_send_and_receive() {
    let loopback_name = find_loopback().expect("No loopback interface found for testing.");

    let (tx, rx) = mpsc::channel::<Vec<u8>>();

    // A unique payload to identify our test frame among other loopback traffic.
    const TEST_PAYLOAD: [u8; 4] = [0xDE, 0xAD, 0xBE, 0xEF];

    // --- Spawn a Receiver Thread ---
    let receiver_thread = thread::spawn(move || {
        let mut receiver_interface = LinuxPnetInterface::new(&loopback_name, 2).unwrap();
        let mut buffer = [0u8; 1518];

        // Loop until we find our specific frame.
        for _ in 0..20 {
            if let Ok(bytes_received) = receiver_interface.receive_frame(&mut buffer) {
                // Check if it's our frame by looking for the EtherType and unique payload.
                if bytes_received >= 18
                    && &buffer[12..14] == &[0x88, 0xAB]
                    && &buffer[14..18] == &TEST_PAYLOAD
                {
                    tx.send(buffer[..bytes_received].to_vec()).unwrap();
                    return;
                }
            }
            thread::sleep(Duration::from_millis(10));
        }
    });

    thread::sleep(Duration::from_millis(50));

    // --- Main Thread Acts as the Sender ---
    let mut sender_interface = LinuxPnetInterface::new("lo", 1).unwrap();

    let mut dummy_frame = [0u8; 60];
    let frame_header: [u8; 14] = [
        0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, // Destination MAC
        0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, // Source MAC
        0x88, 0xAB, // POWERLINK EtherType
    ];
    dummy_frame[..14].copy_from_slice(&frame_header);
    dummy_frame[14..18].copy_from_slice(&TEST_PAYLOAD);

    sender_interface.send_frame(&dummy_frame).unwrap();

    let received_frame = rx
        .recv_timeout(Duration::from_secs(1))
        .expect("Test timed out waiting for the receiver thread to send back the packet.");

    receiver_thread.join().expect("Receiver thread panicked.");

    assert_eq!(dummy_frame.as_slice(), received_frame.as_slice());
}

#[test]
#[ignore] // This test requires root privileges, so ignore it by default.
fn test_cn_responds_to_ident_request() {
    let loopback_name = find_loopback().expect("No loopback interface found for testing.");
    let cn_node_id = 42;
    let placeholder_mac = [0u8; 6];
    let loopback_name_for_thread = loopback_name.clone();

    // --- Setup the Controlled Node in a separate thread ---
    let cn_thread = thread::spawn(move || {
        println!("[CN] Thread started.");
        let mut cn_interface =
            LinuxPnetInterface::new(&loopback_name_for_thread, cn_node_id).unwrap();
        let od = get_test_od(cn_node_id);
        let mut node = ControlledNode::new(od, placeholder_mac.into()).unwrap();
        println!("[CN] Initial NMT state: {:?}", node.nmt_state());

        let mut buffer = [0u8; 1518];
        let mut frames_processed = 0;
        // The CN needs to process 2 frames: SoC, then SoA.
        while frames_processed < 2 {
            match cn_interface.receive_frame(&mut buffer) {
                Ok(bytes) if bytes > 0 => {
                    println!("[CN] Received {} bytes.", bytes);
                    let frame_result = powerlink_rs::deserialize_frame(&buffer[..bytes]);
                    println!("[CN] Deserialized frame: {:?}", frame_result);

                    if let NodeAction::SendFrame(response) =
                        node.process_raw_frame(&buffer[..bytes])
                    {
                        println!("[CN] Sending response of {} bytes.", response.len());
                        cn_interface.send_frame(&response).unwrap();
                    }
                    println!("[CN] New NMT state: {:?}", node.nmt_state());
                    frames_processed += 1;
                }
                _ => {} // Ignore timeouts and empty reads
            }
            thread::sleep(Duration::from_millis(10));
        }
        println!("[CN] Thread finished.");
    });

    thread::sleep(Duration::from_millis(200));

    // --- Setup the "Tester" (simulating the MN) ---
    let mut mn_interface =
        LinuxPnetInterface::new(&loopback_name, C_ADR_MN_DEF_NODE_ID).unwrap();
    let mn_mac = mn_interface.local_mac_address();
    let _cn_mac = mn_mac; // On loopback, the destination is self. Prefix with _ to ignore warning.

    // 1. Send SoC to move CN from NotActive -> PreOperational1
    let net_time = NetTime {
        seconds: 1761159900, // Corresponds to 2025-10-22 13:05:00 in CEST (UTC+2)
        nanoseconds: 123456789,
    };
    let relative_time = RelativeTime {
        seconds: 0,
        nanoseconds: 0,
    };
    let soc_frame = SocFrame::new(mn_mac.into(), Default::default(), net_time, relative_time);
    let mut soc_buffer = vec![0u8; 64];
    let soc_size = soc_frame.serialize(&mut soc_buffer).unwrap();
    soc_buffer.truncate(soc_size);
    println!("[MN] Sending SoC...");
    mn_interface.send_frame(&soc_buffer).unwrap();
    thread::sleep(Duration::from_millis(50));

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

    cn_thread.join().expect("CN thread panicked");

    // 3. Receive the ASnd(IdentResponse) from the CN
    let mut receive_buffer = [0u8; 1518];
    println!("[MN] Waiting for IdentResponse...");
    for i in 0..20 {
        if let Ok(bytes) = mn_interface.receive_frame(&mut receive_buffer) {
            if bytes > 0 {
                println!("[MN] Received {} bytes on attempt {}.", bytes, i);
                if let Ok(PowerlinkFrame::ASnd(asnd)) =
                    powerlink_rs::deserialize_frame(&receive_buffer[..bytes])
                {
                    if asnd.source == NodeId(cn_node_id)
                        && asnd.service_id == ServiceId::IdentResponse
                    {
                        println!("[MN] Success! Received valid IdentResponse.");
                        return; // Test successful!
                    }
                }
            }
        }
        thread::sleep(Duration::from_millis(50));
    }

    panic!("Did not receive a valid ASnd(IdentResponse) frame from the CN.");
}

