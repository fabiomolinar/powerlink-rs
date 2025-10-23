#![cfg(target_os = "linux")]

use powerlink_io_linux::LinuxPnetInterface;
use powerlink_rs::{
    common::{NetTime, RelativeTime},
    frame::{
        PowerlinkFrame, SocFrame, SoAFrame, RequestedServiceId, ServiceId, PReqFrame,
        basic::MacAddress, // Added import for MacAddress
        ASndFrame, // Added import for ASndFrame
    },
    nmt::{flags::FeatureFlags, states::{NmtState}},
    od::{AccessType, Category, Object, ObjectDictionary, ObjectEntry, ObjectValue, PdoMapping},
    pdo::PDOVersion,
    sdo::command::{CommandId, CommandLayerHeader, SdoCommand, Segmentation},
    sdo::sequence::{self, SequenceLayerHeader}, // Added sequence module alias
    types::{NodeId, C_ADR_MN_DEF_NODE_ID, EPLVersion},
    Codec, ControlledNode, NetworkInterface, Node, NodeAction, PowerlinkError,
    deserialize_frame, // Added import for deserialize_frame
};
use pnet::datalink::interfaces;
use std::{env, thread, time::{Duration, Instant}, num::ParseIntError};
use log::{debug, error, info, trace, warn};

// Define constants for clarity
const TEST_INTERFACE: &str = "eth0"; // Standard interface name inside Docker
const FALLBACK_INTERFACE: &str = "lo"; // For local testing
const SLEEP_DURATION: Duration = Duration::from_millis(100); // Slightly longer sleep
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
            default_value: None, value_range: None, pdo_mapping: None,
        },
    );
    od.insert(
        0x1006, // NMT_CycleLen_U32
        ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned32(10000)), // Example: 10ms cycle time
            name: "NMT_CycleLen_U32",
            category: Category::Mandatory,
            access: Some(AccessType::ReadWriteStore),
            default_value: None, value_range: None, pdo_mapping: None,
        },
    );
    od.insert(
        0x1008, // NMT_ManufactDevName_VS
        ObjectEntry {
            object: Object::Variable(ObjectValue::VisibleString("powerlink-rs CN Test".into())),
            name: "NMT_ManufactDevName_VS",
            category: Category::Optional, // Required by IdentResponse logic
            access: Some(AccessType::Constant),
            default_value: None, value_range: None, pdo_mapping: None,
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
            access: None, default_value: None, value_range: None, pdo_mapping: None,
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
            access: None, default_value: None, value_range: None, pdo_mapping: None,
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
            default_value: None, value_range: None, pdo_mapping: None,
        },
    );
    od.insert(
        0x1C14, // DLL_CNLossOfSocTolerance_U32
        ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned32(100000)), // 100 us tolerance
            name: "DLL_CNLossOfSocTolerance_U32",
            category: Category::Mandatory, // Mandatory for CN
            access: Some(AccessType::ReadWriteStore),
            default_value: None, value_range: None, pdo_mapping: None,
        },
    );
    od.insert(
        0x1F99, // NMT_CNBasicEthernetTimeout_U32
        ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned32(5_000_000)), // 5 seconds
            name: "NMT_CNBasicEthernetTimeout_U32",
            category: Category::Mandatory,
            access: Some(AccessType::ReadWriteStore),
            default_value: None, value_range: None, pdo_mapping: None,
        },
    );
    od.insert(
        0x1F8C, // NMT_CurrNMTState_U8 - ReadOnly Mandatory Status Object
        ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned8(NmtState::NmtGsInitialising as u8)),
            name: "NMT_CurrNMTState_U8",
            category: Category::Mandatory,
            access: Some(AccessType::ReadOnly),
            default_value: None, value_range: None, pdo_mapping: Some(PdoMapping::No),
        },
    );
    od.insert(
        0x1F83, // NMT_EPLVersion_U8 - Constant Mandatory Version Object
        ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned8(0x15)), // V1.5
            name: "NMT_EPLVersion_U8",
            category: Category::Mandatory,
            access: Some(AccessType::Constant),
            default_value: None, value_range: None, pdo_mapping: None,
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
            access: None, default_value: None, value_range: None, pdo_mapping: None,
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
            access: None, default_value: None, value_range: None, pdo_mapping: None,
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
            access: None, default_value: None, value_range: None, pdo_mapping: None,
        },
    );
    od.insert(
        0x1E40, // NWL_IpAddrTable_Xh_REC (Conditional, assume supported for test)
        ObjectEntry {
            object: Object::Record(vec![
                ObjectValue::Unsigned16(1),             // 1: IfIndex_U16
                ObjectValue::Unsigned32(0xC0A86400 | node_id as u32), // 2: Addr_IPAD (192.168.100.node_id)
                ObjectValue::Unsigned32(0xFFFFFF00),    // 3: NetMask_IPAD (255.255.255.0)
                ObjectValue::Unsigned16(1500),          // 4: ReasmMaxSize_U16
                ObjectValue::Unsigned32(0xC0A864FE),    // 5: DefaultGateway_IPAD (192.168.100.254)
            ]),
            name: "NWL_IpAddrTable_0h_REC",
            category: Category::Conditional,
            access: None, default_value: None, value_range: None, pdo_mapping: None,
        },
    );
    od.insert(
        0x1F9A, // NMT_HostName_VSTR (Conditional, assume supported for test)
        ObjectEntry {
            object: Object::Variable(ObjectValue::VisibleString(format!("{}-TestHost", node_id))),
            name: "NMT_HostName_VSTR",
            category: Category::Conditional,
            access: Some(AccessType::ReadWriteStore),
            default_value: None, value_range: None, pdo_mapping: None,
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

/// Helper to wait for and receive a specific type of frame.
fn receive_frame_helper<F>(
    interface: &mut LinuxPnetInterface,
    filter_mac: [u8; 6],
    description: &str,
    mut condition: F,
) -> Result<PowerlinkFrame, String>
where
    F: FnMut(PowerlinkFrame) -> Option<PowerlinkFrame>,
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
                        // Clone the frame before passing it to the condition closure
                        let frame_clone = frame.clone();
                        if let Some(matched_frame) = condition(frame_clone) {
                            info!("Received expected {}: {:?}", description, matched_frame);
                            return Ok(matched_frame);
                        } else {
                            // Use the original frame (not moved) for tracing
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
    let mut next_timer_at: Option<Instant> = None;

    loop {
        if start_time.elapsed() > TEST_TIMEOUT {
            error!("[CN] Test timed out.");
            panic!("[CN] Test timed out.");
        }

        // --- Timer Check ---
        let mut tick_needed = false;
        if let Some(expiry_time) = next_timer_at {
            if Instant::now() >= expiry_time {
                tick_needed = true;
                next_timer_at = None; // Consume the timer event
                trace!("[CN] Timer expired, running tick...");
            }
        } else {
            // If no timer is set, run tick occasionally anyway (e.g., every 10ms)
            // to handle non-timer-based state changes or just keep things moving.
            // This is a simplification; a real system might have a more sophisticated scheduler.
            // For now, we rely on receiving frames or explicit timers.
            // tick_needed = true; // Uncomment if periodic ticks are desired
        }

        // --- Tick Execution ---
        if tick_needed {
             let tick_action = node.tick(start_time.elapsed().as_micros() as u64);
             if let NodeAction::SetTimer(delay_us) = tick_action {
                next_timer_at = Some(Instant::now() + Duration::from_micros(delay_us));
                trace!("[CN] Tick requested timer for {} us", delay_us);
             }
        }


        // --- Frame Receiving ---
        match cn_interface.receive_frame(&mut buffer) {
            Ok(bytes) if bytes > 0 => {
                trace!("[CN] Received {} bytes.", bytes);
                let received_slice = &buffer[..bytes];

                if bytes >= 12 && received_slice[6..12] == cn_mac {
                    trace!("[CN] Ignoring received frame from self.");
                    continue;
                }

                let current_time_us = start_time.elapsed().as_micros() as u64;
                let action = node.process_raw_frame(received_slice, current_time_us);
                debug!("[CN] NMT state after processing: {:?}", node.nmt_state());

                if let NodeAction::SendFrame(response) = action {
                    info!("[CN] Sending response ({} bytes)...", response.len());
                    if let Err(e) = cn_interface.send_frame(&response) {
                        error!("[CN] Failed to send response: {:?}", e);
                    }
                } else if let NodeAction::SetTimer(delay_us) = action {
                     next_timer_at = Some(Instant::now() + Duration::from_micros(delay_us));
                     trace!("[CN] Frame processing requested timer for {} us", delay_us);
                }
            }
            Ok(_) => { /* No frame */ }
            Err(PowerlinkError::IoError) => { /* Expected on timeout */ }
            Err(e) => error!("[CN] Receive error: {:?}", e),
        }

        // Determine sleep duration: Sleep until next timer or a short default.
        let sleep_target = next_timer_at.unwrap_or_else(|| Instant::now() + Duration::from_millis(5));
        let now = Instant::now();
        if sleep_target > now {
             thread::sleep(sleep_target - now);
        } else {
             // Timer already expired or no timer set, yield briefly
             thread::sleep(Duration::from_millis(1));
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

    // Give CN time to initialize
    thread::sleep(Duration::from_millis(500));

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
    let _ident_response = receive_frame_helper( // Renamed as value is checked but not used further here
        &mut mn_interface,
        mn_mac,
        "ASnd(IdentResponse)",
        |frame| match frame {
            PowerlinkFrame::ASnd(ref asnd)
                if asnd.source == NodeId(cn_node_id)
                    && asnd.destination == NodeId(C_ADR_MN_DEF_NODE_ID)
                    && asnd.service_id == ServiceId::IdentResponse =>
            {
                Some(frame)
            }
            _ => None,
        },
    )
    .expect("Did not receive IdentResponse");

    // --- Test Specific Logic ---
    match test_to_run {
        "test_cn_responds_to_ident_request" => {
            // Already received IdentResponse, test is successful
            info!("[MN] IdentResponse test successful.");
        }
        "test_cn_responds_to_preq" => {
            run_preq_test_logic(&mut mn_interface, mn_mac, cn_node_id, &soc_buffer);
        }
        "test_sdo_read_by_index_over_asnd" => {
            run_sdo_test_logic(&mut mn_interface, mn_mac, cn_node_id, &soc_buffer);
        }
        _ => panic!("Unknown test case: {}", test_to_run),
    }

    info!(
        "[MN] Test logic completed successfully for '{}'.",
        test_to_run
    );
}

/// MN logic specific to the PReq/PRes test.
fn run_preq_test_logic(
    mn_interface: &mut LinuxPnetInterface,
    mn_mac: [u8; 6],
    cn_node_id: u8,
    soc_buffer: &[u8],
) {
    info!("[MN] Running PReq test logic...");
    // 4. Send second SoC to move CN -> PreOperational2
    send_frame_helper(mn_interface, soc_buffer, "SoC (2nd)");
    thread::sleep(SLEEP_DURATION); // Allow CN to process

    // 5. Send PReq
    let preq = PReqFrame::new(
        mn_mac.into(),
        MacAddress([0x01, 0x11, 0x1E, 0x00, 0x00, cn_node_id]), // Dest MAC based on Node ID (example)
        NodeId(cn_node_id),
        Default::default(), // Flags: MS=0, EA=0, RD=0 (PreOp2)
        PDOVersion(0),
        Vec::new(), // Empty payload for this test
    );
    let mut preq_buffer = vec![0u8; 64];
    let preq_size = preq.serialize(&mut preq_buffer).unwrap();
    preq_buffer.truncate(preq_size);
    send_frame_helper(mn_interface, &preq_buffer, "PReq");

    // 6. Receive PRes
    receive_frame_helper(
        mn_interface,
        mn_mac,
        "PRes",
        |frame| match frame {
            PowerlinkFrame::PRes(ref pres)
                if pres.source == NodeId(cn_node_id)
                    && pres.destination == NodeId(powerlink_rs::types::C_ADR_BROADCAST_NODE_ID) =>
            {
                // Basic check: Ensure it's a PRes from the right CN
                Some(frame)
            }
            _ => None,
        },
    )
    .expect("Did not receive PRes");
    info!("[MN] PReq test successful.");
}

/// MN logic specific to the SDO test.
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

    // 5. Send ASnd(SDO Read Request) for 0x1008/0 (Device Name)
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
    let seq_header = SequenceLayerHeader {
        // Assuming we need to initialize the connection first
        send_con: sequence::SendConnState::Initialization,
        receive_con: sequence::ReceiveConnState::NoConnection,
        ..Default::default()
    };

    let mut sdo_asnd_payload = vec![0u8; 1500]; // Buffer for ASnd payload (SDO msg)
    let sdo_payload_len = {
        let mut offset = 0;
        offset += seq_header.serialize(&mut sdo_asnd_payload[offset..]).unwrap();
        offset += sdo_read_cmd
            .serialize(&mut sdo_asnd_payload[offset..])
            .unwrap();
        offset
    };
    sdo_asnd_payload.truncate(sdo_payload_len);

    let asnd_sdo_req = ASndFrame::new(
        mn_mac.into(),
        MacAddress([0x01, 0x11, 0x1E, 0x00, 0x00, cn_node_id]), // Dest MAC based on Node ID
        NodeId(cn_node_id),
        NodeId(C_ADR_MN_DEF_NODE_ID),
        ServiceId::Sdo,
        sdo_asnd_payload,
    );

    let mut asnd_buffer = vec![0u8; 1500];
    let asnd_size = asnd_sdo_req.serialize(&mut asnd_buffer).unwrap();
    asnd_buffer.truncate(asnd_size);
    send_frame_helper(mn_interface, &asnd_buffer, "ASnd(SDO Read Request)");

    // 6. Receive ASnd(SDO Response)
    let sdo_response_frame = receive_frame_helper(
        mn_interface,
        mn_mac,
        "ASnd(SDO Response)",
        |frame| match frame {
            PowerlinkFrame::ASnd(ref asnd)
                if asnd.source == NodeId(cn_node_id)
                    && asnd.destination == NodeId(C_ADR_MN_DEF_NODE_ID)
                    && asnd.service_id == ServiceId::Sdo =>
            {
                // Check if it's an SDO response (skip Seq Hdr)
                if asnd.payload.len() >= 8 { // Min size for SDO Seq + Cmd header
                   match SdoCommand::deserialize(&asnd.payload[4..]) { // Skip Seq header
                       Ok(cmd) if cmd.header.is_response && !cmd.header.is_aborted => Some(frame),
                       Ok(cmd) if cmd.header.is_aborted => {
                           warn!("Received SDO Abort: {:?}", cmd); None
                       }
                       Err(e) => {
                           warn!("Failed to deserialize SDO command in ASnd: {:?}", e); None
                       }
                       _ => None,
                   }
                } else { None }
            }
            _ => None,
        },
    )
    .expect("Did not receive ASnd(SDO Response)");

    // 7. Validate SDO Response Payload
    if let PowerlinkFrame::ASnd(asnd) = sdo_response_frame {
        // Need to deserialize Seq Header too to get offset right for Cmd
        let _seq_header = SequenceLayerHeader::deserialize(&asnd.payload[0..4]).unwrap(); // Assign to _ to avoid unused warning
        let sdo_cmd = SdoCommand::deserialize(&asnd.payload[4..]).unwrap(); // Skip Seq Hdr
        let expected_name = "powerlink-rs CN Test";
        assert_eq!(sdo_cmd.payload, expected_name.as_bytes());
        info!("[MN] SDO Read test successful. Received name: {}", expected_name);
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
fn test_cn_responds_to_preq() {
    run_test_logic("test_cn_responds_to_preq");
}

#[test]
#[ignore]
fn test_sdo_read_by_index_over_asnd() {
    run_test_logic("test_sdo_read_by_index_over_asnd");
}

