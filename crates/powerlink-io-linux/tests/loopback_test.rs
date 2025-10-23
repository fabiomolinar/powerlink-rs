#![cfg(target_os = "linux")]

use powerlink_io_linux::LinuxPnetInterface;
use powerlink_rs::{
    common::{NetTime, RelativeTime},
    frame::{
        basic::MacAddress, ASndFrame, Codec, PowerlinkFrame, RequestedServiceId, ServiceId,
        SoAFrame, SocFrame,
    },
    nmt::{flags::FeatureFlags, states::NmtState},
    od::{AccessType, Category, Object, ObjectDictionary, ObjectEntry, ObjectValue, PdoMapping},
    // Removed unused imports PReqFrame and PDOVersion
    sdo::{
        command::{CommandId, CommandLayerHeader, SdoCommand, Segmentation},
        sequence::{ReceiveConnState, SendConnState, SequenceLayerHeader},
    },
    types::{NodeId, C_ADR_MN_DEF_NODE_ID, EPLVersion},
    deserialize_frame, ControlledNode, NetworkInterface, Node, NodeAction, PowerlinkError,
};
use pnet::datalink::interfaces;
use std::{
    env,
    num::ParseIntError,
    thread,
    time::{Duration, Instant},
};
use log::{debug, error, info, trace, warn};

// Define constants for clarity
const TEST_INTERFACE: &str = "eth0"; // Standard interface name inside Docker
const FALLBACK_INTERFACE: &str = "lo"; // For local testing
const SLEEP_DURATION: Duration = Duration::from_millis(200); // Increased sleep duration slightly
const TEST_TIMEOUT: Duration = Duration::from_secs(20); // Overall test timeout

/// Helper function to find the network interface for testing.
/// Prefers "eth0" (common in Docker) but falls back to "lo" (loopback).
fn find_test_interface() -> String {
    interfaces()
        .into_iter()
        .find(|iface| iface.name == TEST_INTERFACE)
        .map(|iface| iface.name.clone())
        .unwrap_or_else(|| {
            warn!("Interface 'eth0' not found, falling back to 'lo'.");
            FALLBACK_INTERFACE.to_string()
        })
}

/// Helper function to get Node ID from environment variable.
fn get_cn_node_id() -> Result<u8, ParseIntError> {
    env::var("POWERLINK_CN_NODE_ID")
        .expect("Missing POWERLINK_CN_NODE_ID environment variable")
        .parse()
}

/// Helper function to create a minimal but valid Object Dictionary for a CN.
fn get_test_od(node_id: u8) -> ObjectDictionary<'static> {
    let mut od = ObjectDictionary::new(None);

    // Mandatory/Required objects for initialization and IdentResponse, based on DS 301.
    od.insert(
        0x1000, // NMT_DeviceType_U32
        ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned32(0x000F0191)), // Example: Generic I/O Device
            name: "NMT_DeviceType_U32",
            category: Category::Mandatory,
            access: Some(AccessType::Constant),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );
    od.insert(
        0x1006, // NMT_CycleLen_U32
        ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned32(10000)), // Example: 10ms cycle time
            name: "NMT_CycleLen_U32",
            category: Category::Mandatory,
            access: Some(AccessType::ReadWriteStore),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );
    od.insert(
        0x1008, // NMT_ManufactDevName_VS
        ObjectEntry {
            object: Object::Variable(ObjectValue::VisibleString("powerlink-rs CN Test".into())),
            name: "NMT_ManufactDevName_VS",
            category: Category::Optional, // Required by IdentResponse logic
            access: Some(AccessType::Constant),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );
    od.insert(
        0x1018, // NMT_IdentityObject_REC
        ObjectEntry {
            object: Object::Record(vec![
                ObjectValue::Unsigned32(0x12345678), // VendorId
                ObjectValue::Unsigned32(0x00000001), // ProductCode
                ObjectValue::Unsigned32(0x00010000), // RevisionNo (Maj.Min)
                ObjectValue::Unsigned32(0xABCDEF01), // SerialNo
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
        0x1F93, // NMT_EPLNodeID_REC
        ObjectEntry {
            object: Object::Record(vec![
                ObjectValue::Unsigned8(node_id),
                ObjectValue::Boolean(0), // Node ID by HW = FALSE
            ]),
            name: "NMT_EPLNodeID_REC",
            category: Category::Mandatory,
            access: None,
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );
    // FeatureFlags: Isochronous + SDO/UDP + SDO/ASnd
    let flags = FeatureFlags::ISOCHRONOUS | FeatureFlags::SDO_ASND | FeatureFlags::SDO_UDP;
    od.insert(
        0x1F82, // NMT_FeatureFlags_U32
        ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned32(flags.0)),
            name: "NMT_FeatureFlags_U32",
            category: Category::Mandatory,
            access: Some(AccessType::Constant),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );
    od.insert(
        0x1C14, // DLL_CNLossOfSocTolerance_U32
        ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned32(100000)), // 100 us tolerance
            name: "DLL_CNLossOfSocTolerance_U32",
            category: Category::Mandatory, // Mandatory for CN
            access: Some(AccessType::ReadWriteStore),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );
    od.insert(
        0x1F99, // NMT_CNBasicEthernetTimeout_U32
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
            default_value: None,
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
    od.insert(
        0x1F98, // NMT_CycleTiming_REC
        ObjectEntry {
            object: Object::Record(vec![
                ObjectValue::Unsigned16(1490), // 1: IsochrTxMaxPayload_U16
                ObjectValue::Unsigned16(1490), // 2: IsochrRxMaxPayload_U16
                ObjectValue::Unsigned32(10000), // 3: PresMaxLatency_U32 (10 us)
                ObjectValue::Unsigned16(100),  // 4: PreqActPayloadLimit_U16
                ObjectValue::Unsigned16(100),  // 5: PresActPayloadLimit_U16
                ObjectValue::Unsigned32(20000), // 6: AsndMaxLatency_U32 (20 us)
                ObjectValue::Unsigned8(0),     // 7: MultiplCycleCnt_U8
                ObjectValue::Unsigned16(300),  // 8: AsyncMTU_U16
                ObjectValue::Unsigned16(2),    // 9: Prescaler_U16
            ]),
            name: "NMT_CycleTiming_REC",
            category: Category::Mandatory,
            access: None,
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );
    od.insert(
        0x1020, // CFM_VerifyConfiguration_REC
        ObjectEntry {
            object: Object::Record(vec![
                ObjectValue::Unsigned32(0), // 1: ConfDate_U32
                ObjectValue::Unsigned32(0), // 2: ConfTime_U32
            ]),
            name: "CFM_VerifyConfiguration_REC",
            category: Category::Mandatory,
            access: None,
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );
    od.insert(
        0x1F52, // PDL_LocVerApplSw_REC (Conditional, assume supported for test)
        ObjectEntry {
            object: Object::Record(vec![
                ObjectValue::Unsigned32(0), // 1: ApplSwDate_U32
                ObjectValue::Unsigned32(0), // 2: ApplSwTime_U32
            ]),
            name: "PDL_LocVerApplSw_REC",
            category: Category::Conditional,
            access: None,
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );
    od.insert(
        0x1E40, // NWL_IpAddrTable_Xh_REC (Conditional, assume supported for test)
        ObjectEntry {
            object: Object::Record(vec![
                ObjectValue::Unsigned16(1), // 1: IfIndex_U16
                ObjectValue::Unsigned32(0xC0A86400 | node_id as u32), // 2: Addr_IPAD (192.168.100.node_id)
                ObjectValue::Unsigned32(0xFFFFFF00), // 3: NetMask_IPAD (255.255.255.0)
                ObjectValue::Unsigned16(1500),       // 4: ReasmMaxSize_U16
                ObjectValue::Unsigned32(0xC0A864FE), // 5: DefaultGateway_IPAD (192.168.100.254)
            ]),
            name: "NWL_IpAddrTable_0h_REC",
            category: Category::Conditional,
            access: None,
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );
    od.insert(
        0x1F9A, // NMT_HostName_VSTR (Conditional, assume supported for test)
        ObjectEntry {
            object: Object::Variable(ObjectValue::VisibleString(format!("{}-TestHost", node_id))),
            name: "NMT_HostName_VSTR",
            category: Category::Conditional,
            access: Some(AccessType::ReadWriteStore),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );

    od
}

/// Common setup for interface and node creation.
fn setup_node<'a>(
    interface_name: &str,
    node_id: u8,
    od: ObjectDictionary<'a>,
) -> (LinuxPnetInterface, ControlledNode<'a>) {
    // Interface does not need to be mutable here
    let interface = match LinuxPnetInterface::new(interface_name, node_id) {
        Ok(iface) => iface,
        Err(e) => {
            error!("Failed to create interface '{}': {}", interface_name, e);
            panic!("Interface creation failed.");
        }
    };
    let mac = interface.local_mac_address();
    let node = match ControlledNode::new(od, mac.into()) {
        Ok(n) => n,
        Err(e) => {
            error!("Failed to create ControlledNode: {:?}", e);
            panic!("Node creation failed.");
        }
    };
    (interface, node)
}

/// Helper to send a frame and handle potential errors.
fn send_frame_helper(interface: &mut LinuxPnetInterface, frame_bytes: &[u8], frame_name: &str) {
    info!("[MN] Sending {}...", frame_name);
    if let Err(e) = interface.send_frame(frame_bytes) {
        error!("[MN] Failed to send {}: {:?}", frame_name, e);
        panic!("Failed to send {}", frame_name);
    }
}

/// Helper function to build SDO payload.
/// Builds the SDO payload (Seq Hdr + Cmd Hdr + Cmd Payload)
fn build_sdo_payload(seq_header: SequenceLayerHeader, cmd: SdoCommand) -> Vec<u8> {
    let mut payload = vec![0u8; 1500]; // Use a large enough buffer
    let mut offset = 0;
    // Serialize Sequence Layer Header (4 bytes)
    offset += seq_header
        .serialize(&mut payload[offset..offset + 4])
        .unwrap_or_else(|e| {
            error!("Error serializing Seq Header: {:?}", e);
            0
        });
    // Serialize Command Layer (Header + Payload)
    offset += cmd.serialize(&mut payload[offset..]).unwrap_or_else(|e| {
        error!("Error serializing SDO Command: {:?}", e);
        0
    });
    payload.truncate(offset); // Truncate to actual size
    payload
}

/// Helper to wait for and receive a specific type of frame.
/// Now accepts a closure taking a reference to the frame.
fn receive_frame_helper<F>(
    interface: &mut LinuxPnetInterface,
    filter_mac: [u8; 6],
    description: &str,
    mut condition: F,
) -> Result<PowerlinkFrame, String>
where
    F: FnMut(&PowerlinkFrame) -> bool, // Closure now takes a reference and returns bool
{
    let mut buffer = [0u8; 1518];
    let start_time = Instant::now();
    info!("Waiting for {}...", description);

    while start_time.elapsed() < TEST_TIMEOUT {
        match interface.receive_frame(&mut buffer) {
            Ok(bytes) if bytes > 0 => {
                let received_slice = &buffer[..bytes];
                // Ignore self-sent frames
                if bytes >= 12 && received_slice[6..12] == filter_mac {
                    continue;
                }
                match deserialize_frame(received_slice) {
                    Ok(frame) => {
                        // Pass a reference to the closure
                        if condition(&frame) {
                            info!("Received expected {}: {:?}", description, frame);
                            return Ok(frame); // Return the original frame
                        } else {
                            trace!("Received other frame: {:?}", frame);
                        }
                    }
                    Err(PowerlinkError::InvalidEthernetFrame) => {
                        trace!("Received non-POWERLINK frame.");
                    }
                    Err(e) => {
                        warn!("Error deserializing received frame: {:?}", e);
                    }
                }
            }
            Ok(_) => { /* No frame, continue */ }
            Err(PowerlinkError::IoError) => { /* Expected on timeout */ }
            Err(e) => {
                error!("Receive error: {:?}", e);
            }
        }
        thread::sleep(Duration::from_millis(10)); // Prevent busy-wait
    }
    Err(format!("Timeout waiting for {}", description))
}

/// Logic for the Controlled Node (CN) role in the test.
fn run_cn_logic(interface_name: &str) {
    env_logger::try_init().ok();
    let cn_node_id = get_cn_node_id().expect("Failed to parse CN Node ID from environment");
    info!(
        "[CN] Thread started. Node ID: {}. Interface: {}",
        cn_node_id, interface_name
    );

    let od = get_test_od(cn_node_id);
    let (mut cn_interface, mut node) = setup_node(interface_name, cn_node_id, od);
    let cn_mac = cn_interface.local_mac_address();

    info!("[CN] Initial NMT state: {:?}", node.nmt_state());

    let mut buffer = [0u8; 1518];
    let start_time = Instant::now();
    let mut next_action = NodeAction::NoAction; // Store action from tick/process

    loop {
        if start_time.elapsed() > TEST_TIMEOUT {
            error!("[CN] Test timed out.");
            panic!("[CN] Test timed out.");
        }

        let current_time_us = start_time.elapsed().as_micros() as u64;

        // --- Frame Receiving First ---
        match cn_interface.receive_frame(&mut buffer) {
            Ok(bytes) if bytes > 0 => {
                trace!("[CN] Received {} bytes.", bytes);
                let received_slice = &buffer[..bytes];

                if bytes >= 12 && received_slice[6..12] == cn_mac {
                    trace!("[CN] Ignoring received frame from self.");
                    continue;
                }

                let frame_action = node.process_raw_frame(received_slice, current_time_us);
                debug!(
                    "[CN] NMT state after processing frame: {:?}",
                    node.nmt_state()
                );

                if frame_action != NodeAction::NoAction {
                    next_action = frame_action; // Prioritize frame action
                }
            }
            Ok(_) => { /* No frame */ }
            Err(PowerlinkError::IoError) => { /* Expected on timeout */ }
            Err(e) => error!("[CN] Receive error: {:?}", e),
        }

        // --- Execute Pending Actions ---
        match next_action {
            NodeAction::SendFrame(response) => {
                info!("[CN] Sending response ({} bytes)...", response.len());
                if let Err(e) = cn_interface.send_frame(&response) {
                    error!("[CN] Failed to send response: {:?}", e);
                }
                next_action = NodeAction::NoAction; // Action handled
            }
            NodeAction::SetTimer(delay_us) => {
                // pnet's timeout handles this, but we use it to avoid busy-wait
                // Only sleep if the delay is significant to avoid tiny sleeps
                let sleep_duration = Duration::from_micros(delay_us.min(100_000)); // Sleep at most 100ms
                if sleep_duration > Duration::from_millis(1) {
                    trace!("[CN] Sleeping for {:?}", sleep_duration);
                    thread::sleep(sleep_duration);
                }
                next_action = NodeAction::NoAction; // Timer "event" implicitly handled by waking up
            }
            NodeAction::NoAction => {
                // No action from frame processing, check for tick actions
                let tick_action = node.tick(current_time_us);
                if tick_action != NodeAction::NoAction {
                    next_action = tick_action; // Prioritize tick action
                    debug!(
                        "[CN] NMT state after tick: {:?}",
                        node.nmt_state()
                    );
                } else {
                    // No frame and no tick action, sleep briefly
                    thread::sleep(Duration::from_millis(2));
                }
            }
        }
    }
}

/// Logic for the Managing Node (MN) role in the test.
fn run_mn_logic(interface_name: &str, test_to_run: &str) {
    env_logger::try_init().ok();
    let cn_node_id = get_cn_node_id().expect("Failed to parse CN Node ID from environment");
    info!(
        "[MN] Starting MN logic. Target CN ID: {}. Interface: {}. Test: {}",
        cn_node_id, interface_name, test_to_run
    );

    let mut mn_interface =
        match LinuxPnetInterface::new(interface_name, C_ADR_MN_DEF_NODE_ID) {
            Ok(iface) => iface,
            Err(e) => {
                error!("[MN] Failed to create interface: {}", e);
                panic!("[MN] Interface creation failed.");
            }
        };
    let mn_mac = mn_interface.local_mac_address();

    // Give CN time to initialize (Increased slightly)
    thread::sleep(Duration::from_millis(700));

    // --- Common Boot Sequence ---
    // 1. Send SoC to move CN from NotActive -> PreOperational1
    let net_time = NetTime {
        seconds: 0,
        nanoseconds: 0,
    };
    let relative_time = RelativeTime {
        seconds: 0,
        nanoseconds: 0,
    };
    let soc_frame = SocFrame::new(mn_mac.into(), Default::default(), net_time, relative_time);
    let mut soc_buffer = vec![0u8; 64];
    let soc_size = soc_frame.serialize(&mut soc_buffer).unwrap();
    soc_buffer.truncate(soc_size);
    send_frame_helper(&mut mn_interface, &soc_buffer, "SoC (1st)");
    thread::sleep(SLEEP_DURATION); // Allow CN to process

    // 2. Send SoA(IdentRequest)
    let soa = SoAFrame::new(
        mn_mac.into(),
        NmtState::NmtPreOperational1,
        Default::default(),
        RequestedServiceId::IdentRequest,
        NodeId(cn_node_id),
        EPLVersion(0x15),
    );
    let mut soa_buffer = vec![0u8; 64];
    let soa_size = soa.serialize(&mut soa_buffer).unwrap();
    soa_buffer.truncate(soa_size);
    send_frame_helper(&mut mn_interface, &soa_buffer, "SoA(IdentRequest)");

    // 3. Receive ASnd(IdentResponse)
    let _ident_response = receive_frame_helper(
        // Renamed as value is checked but not used further here
        &mut mn_interface,
        mn_mac,
        "ASnd(IdentResponse)",
        |frame| match frame {
            // Closure now takes a reference
            // Remove 'ref' here due to Rust 2021 match ergonomics
            PowerlinkFrame::ASnd(asnd)
                if asnd.source == NodeId(cn_node_id)
                    && asnd.destination == NodeId(C_ADR_MN_DEF_NODE_ID)
                    && asnd.service_id == ServiceId::IdentResponse =>
            {
                true // Return bool
            }
            _ => false,
        },
    )
    .expect("Did not receive IdentResponse");

    // --- Test Specific Logic ---
    match test_to_run {
        "test_cn_responds_to_ident_request" => {
            // Already received IdentResponse, test is successful
            info!("[MN] IdentResponse test successful.");
        }
        "test_sdo_read_by_index_over_asnd" => {
            run_sdo_test_logic(&mut mn_interface, mn_mac, cn_node_id, &soc_buffer);
        }
        // "test_cn_responds_to_preq" arm removed
        _ => panic!("Unknown test case: {}", test_to_run),
    }

    info!(
        "[MN] Test logic completed successfully for '{}'.",
        test_to_run
    );
}

/// MN logic specific to the SDO test. Corrects the SDO handshake.
fn run_sdo_test_logic(
    mn_interface: &mut LinuxPnetInterface,
    mn_mac: [u8; 6],
    cn_node_id: u8,
    soc_buffer: &[u8],
) {
    info!("[MN] Running SDO test logic...");
    // 4. Send second SoC to move CN -> PreOperational2 (Needed before SDO usually)
    send_frame_helper(mn_interface, soc_buffer, "SoC (2nd for SDO)");
    thread::sleep(SLEEP_DURATION);

    // --- SDO Handshake ---
    let mut current_mn_sdo_seq: u8 = 0;
    let mut last_acked_cn_sdo_seq: u8 = 63; // Start at 63 (equiv. to -1)

    // 5a. Send ASnd(SDO Init Request)
    let init_cmd = SdoCommand {
        // Empty command for init
        header: Default::default(),
        data_size: None,
        payload: Vec::new(),
    };
    let init_seq_header = SequenceLayerHeader {
        send_sequence_number: current_mn_sdo_seq,
        send_con: SendConnState::Initialization,
        receive_sequence_number: last_acked_cn_sdo_seq, // Ack CN's initial (non-existent) seq
        receive_con: ReceiveConnState::NoConnection,
    };
    // Use the local build_sdo_payload function
    let init_sdo_payload = build_sdo_payload(init_seq_header, init_cmd.clone());
    let init_asnd = ASndFrame::new(
        mn_mac.into(),
        MacAddress([0x01, 0x11, 0x1E, 0x00, 0x00, cn_node_id]),
        NodeId(cn_node_id),
        NodeId(C_ADR_MN_DEF_NODE_ID),
        ServiceId::Sdo,
        init_sdo_payload,
    );
    let mut init_asnd_buffer = vec![0u8; 1500];
    let init_asnd_size = init_asnd.serialize(&mut init_asnd_buffer).unwrap();
    init_asnd_buffer.truncate(init_asnd_size);
    send_frame_helper(
        mn_interface,
        &init_asnd_buffer,
        "ASnd(SDO Init Request)",
    );

    // 5b. Receive ASnd(SDO Init ACK)
    let _init_ack_frame = receive_frame_helper(
        // Assign to _
        mn_interface,
        mn_mac,
        "ASnd(SDO Init ACK)",
        |frame| match frame {
            // Closure now takes a reference
            // Remove 'ref' here due to Rust 2021 match ergonomics
            PowerlinkFrame::ASnd(asnd) if asnd.service_id == ServiceId::Sdo => {
                // SDO Payload starts at offset 0 (Seq Header) of ASnd payload
                SequenceLayerHeader::deserialize(&asnd.payload[0..4])
                    .ok()
                    .and_then(|seq| {
                        if seq.send_con == SendConnState::Initialization
                            && seq.receive_sequence_number == current_mn_sdo_seq
                        {
                            last_acked_cn_sdo_seq = seq.send_sequence_number; // Store CN's sequence number
                            Some(true) // Return Option<bool>
                        } else {
                            None
                        } // Mismatch, not the frame we want
                    })
                    .unwrap_or(false) // Convert Option<bool> to bool for condition fn
            }
            _ => false,
        },
    )
    .expect("Did not receive SDO Init ACK");

    current_mn_sdo_seq = current_mn_sdo_seq.wrapping_add(1) % 64; // Increment MN sequence number

    // 5c. Send ASnd(SDO Read Request + Valid ACK)
    let sdo_read_cmd = SdoCommand {
        header: CommandLayerHeader {
            transaction_id: 1, // Example transaction ID
            is_response: false,
            is_aborted: false,
            segmentation: Segmentation::Expedited,
            command_id: CommandId::ReadByIndex,
            segment_size: 4, // Size of index/subindex payload
        },
        data_size: None,
        payload: vec![0x08, 0x10, 0x00, 0x00], // Index 0x1008, Sub-index 0
    };
    let read_seq_header = SequenceLayerHeader {
        send_sequence_number: current_mn_sdo_seq,
        send_con: SendConnState::ConnectionValid,
        receive_sequence_number: last_acked_cn_sdo_seq, // ACK CN's Init ACK sequence number
        receive_con: ReceiveConnState::ConnectionValid,
    };
    // Use the local build_sdo_payload function
    let read_sdo_payload = build_sdo_payload(read_seq_header, sdo_read_cmd);
    let read_asnd = ASndFrame::new(
        mn_mac.into(),
        MacAddress([0x01, 0x11, 0x1E, 0x00, 0x00, cn_node_id]),
        NodeId(cn_node_id),
        NodeId(C_ADR_MN_DEF_NODE_ID),
        ServiceId::Sdo,
        read_sdo_payload,
    );
    let mut read_asnd_buffer = vec![0u8; 1500];
    let read_asnd_size = read_asnd.serialize(&mut read_asnd_buffer).unwrap();
    read_asnd_buffer.truncate(read_asnd_size);
    send_frame_helper(
        mn_interface,
        &read_asnd_buffer,
        "ASnd(SDO Read Request + Valid ACK)",
    );

    // 6. Receive ASnd(SDO Data Response)
    let sdo_response_frame = receive_frame_helper(
        mn_interface,
        mn_mac,
        "ASnd(SDO Data Response)",
        |frame| match frame {
            // Closure now takes a reference
            // Remove 'ref' here due to Rust 2021 match ergonomics
            PowerlinkFrame::ASnd(asnd) if asnd.service_id == ServiceId::Sdo => {
                // SDO Payload starts at offset 0 (Seq Header) of ASnd payload
                SequenceLayerHeader::deserialize(&asnd.payload[0..4])
                    .ok()
                    .and_then(|seq| {
                        if seq.receive_sequence_number == current_mn_sdo_seq {
                            // Check if it ACKs our Read Request
                            last_acked_cn_sdo_seq = seq.send_sequence_number; // Store CN's sequence number
                                                                              // Now deserialize the command part (starts after Seq Header)
                            SdoCommand::deserialize(&asnd.payload[4..]).ok().and_then(
                                |cmd| {
                                    if cmd.header.is_response && !cmd.header.is_aborted {
                                        Some(true) // Return Option<bool>
                                    } else {
                                        warn!("Received SDO Abort or non-response cmd: {:?}", cmd);
                                        None // Not the frame we want
                                    }
                                },
                            )
                        } else {
                            None
                        } // Not the frame we want
                    })
                    .unwrap_or(false) // Convert Option<bool> to bool for condition fn
            }
            _ => false,
        },
    )
    .expect("Did not receive valid SDO Data Response");

    // 7. Validate SDO Response Payload
    if let PowerlinkFrame::ASnd(asnd) = sdo_response_frame {
        // SDO Command starts after Seq Header (4 bytes)
        let sdo_cmd = SdoCommand::deserialize(&asnd.payload[4..]).unwrap();
        let expected_name = "powerlink-rs CN Test";
        assert_eq!(sdo_cmd.payload, expected_name.as_bytes());
        info!(
            "[MN] SDO Read test successful. Received name: {}",
            expected_name
        );
    } else {
        panic!("Received frame was not an ASnd");
    }
}

/// Dispatches based on the `POWERLINK_TEST_ROLE` env var.
fn run_test_logic(test_name: &str) {
    let test_interface_name = find_test_interface();
    let role = env::var("POWERLINK_TEST_ROLE").unwrap_or_else(|_| "MN".to_string());

    if role == "CN" {
        run_cn_logic(&test_interface_name);
    } else {
        run_mn_logic(&test_interface_name, test_name);
    }
}

#[test]
#[ignore]
fn test_cn_responds_to_ident_request() {
    run_test_logic("test_cn_responds_to_ident_request");
}

#[test]
#[ignore]
fn test_sdo_read_by_index_over_asnd() {
    run_test_logic("test_sdo_read_by_index_over_asnd");
}

