#![cfg(target_os = "linux")]

use powerlink_io_linux::LinuxPnetInterface;
use powerlink_rs::{
    frame::{PReqFrame, PReqFlags, PowerlinkFrame},
    nmt::flags::FeatureFlags,
    od::{AccessType, Category, Object, ObjectDictionary, ObjectEntry, ObjectValue, PdoMapping},
    pdo::PDOVersion,
    types::{NodeId, C_ADR_MN_DEF_NODE_ID},
    Codec, ControlledNode, Node, NodeAction,
};
use pnet::datalink::interfaces;
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

    od
}

#[test]
#[ignore] // This test requires root privileges, so ignore it by default.
fn test_cn_responds_to_preq_on_loopback() {
    let loopback_name = find_loopback().expect("No loopback interface found for testing.");
    let cn_node_id = 42;

    // The MAC address for loopback is usually 00:00:00:00:00:00.
    // We use a placeholder here as pnet will get the real one.
    let placeholder_mac = [0u8; 6];

    // --- Setup the Controlled Node in a separate thread ---
    let cn_thread = thread::spawn(move || {
        let mut cn_interface = LinuxPnetInterface::new(&loopback_name, cn_node_id).unwrap();
        let od = get_test_od(cn_node_id);

        let mut node = ControlledNode::new(od, placeholder_mac.into()).unwrap();

        let mut buffer = [0u8; 1518];
        // Wait for one frame, process it, and send the response.
        // Loop briefly to handle potential timeouts from the interface.
        for _ in 0..5 {
            if let Ok(bytes) = cn_interface.receive_frame(&mut buffer) {
                if bytes > 0 {
                    if let NodeAction::SendFrame(response) = node.process_raw_frame(&buffer[..bytes]) {
                        cn_interface.send_frame(&response).unwrap();
                    }
                    return; // Exit thread after handling the frame.
                }
            }
            thread::sleep(Duration::from_millis(50));
        }
        panic!("CN thread timed out waiting for a frame.");
    });

    // Give the CN thread a moment to initialize the interface.
    thread::sleep(Duration::from_millis(200));

    // --- Setup the "Tester" (simulating the MN) ---
    let mut mn_interface =
        LinuxPnetInterface::new(&loopback_name, C_ADR_MN_DEF_NODE_ID).unwrap();
    let mn_mac = mn_interface.local_mac_address();
    // On loopback, the destination MAC is often the same as the source.
    let cn_mac = mn_mac;

    // Create and serialize a PReq frame.
    let preq = PReqFrame::new(
        mn_mac.into(),
        cn_mac.into(),
        NodeId(cn_node_id),
        PReqFlags { rd: true, ..Default::default() },
        PDOVersion(0),
        vec![0x01, 0x02],
    );
    let mut send_buffer = vec![0u8; 64]; // Ensure at least minimum Ethernet size.
    let size = preq.serialize(&mut send_buffer).unwrap();
    send_buffer.truncate(size);

    // Send the PReq.
    mn_interface.send_frame(&send_buffer).unwrap();

    // Wait for the CN to process and send its response.
    cn_thread.join().expect("CN thread panicked");

    // Receive the response on the MN's interface. Loop to handle timeouts.
    let mut receive_buffer = [0u8; 1518];
    for _ in 0..5 {
        if let Ok(bytes) = mn_interface.receive_frame(&mut receive_buffer) {
            if bytes > 0 {
                if let Ok(PowerlinkFrame::PRes(pres)) = powerlink_rs::deserialize_frame(&receive_buffer[..bytes]) {
                    // Assert that we received a valid PRes from the correct CN.
                    assert_eq!(pres.source, NodeId(cn_node_id));
                    return; // Test successful!
                }
            }
        }
        thread::sleep(Duration::from_millis(50));
    }

    panic!("Did not receive a valid PRes frame from the CN.");
}