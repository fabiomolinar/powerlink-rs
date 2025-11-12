// crates/powerlink-rs-linux/examples/io_module.rs
//! A full-system example demonstrating a POWERLINK network with one
//! Managing Node (MN) and one Controlled Node (CN) acting as a simple I/O module.
//!
//! To run this example:
//! 1. Ensure you have Docker and docker-compose installed.
//! 2. From the workspace root, run:
//!    docker-compose -f crates/powerlink-rs-linux/examples/io_module_resources/docker-compose.yml up --build
//!
//! The MN will print the digital inputs it receives from the CN, and the CN will
//! print the digital outputs it receives from the MN. The MN logic mirrors the
//! CN's inputs back to its outputs.

use log::{error, info};
use powerlink_rs::{
    ControlledNode,
    NetworkInterface,
    PowerlinkError,
    frame::basic::MacAddress,
    nmt::{flags::FeatureFlags, states::NmtState},
    node::{ManagingNode, Node, NodeAction}, // Corrected import
    od::{AccessType, Category, Object, ObjectDictionary, ObjectEntry, ObjectValue, PdoMapping},
    pdo::PdoMappingEntry,
    types::{C_ADR_MN_DEF_NODE_ID, IpAddress}, // Import IpAddress for UDP
};
use powerlink_rs_linux::LinuxPnetInterface;
use std::{
    env, thread,
    time::{Duration, Instant},
};

// --- Common Configuration (Moved to top level) ---

const CN_NODE_ID: u8 = 42;
const CYCLE_TIME_US: u32 = 20_000; // 20ms

// OD indices for our I/O module's application data
const IDX_DIGITAL_INPUTS: u16 = 0x6000;
const IDX_ANALOG_INPUTS: u16 = 0x6001;
const IDX_DIGITAL_OUTPUTS: u16 = 0x6200;
const IDX_ANALOG_OUTPUTS: u16 = 0x6201;

/// Creates the Object Dictionary for the I/O module CN.
fn get_cn_od(node_id: u8) -> ObjectDictionary<'static> {
    let mut od = ObjectDictionary::new(None);

    // --- Mandatory Communication Objects (abbreviated) ---
    add_mandatory_cn_objects(&mut od, node_id);

    // --- Application Data Objects ---
    // Inputs (data source on CN)
    od.insert(
        IDX_DIGITAL_INPUTS,
        ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned8(0)),
            name: "Digital_Inputs_8bit",
            category: Category::Mandatory,
            access: Some(AccessType::ReadWrite), // Application writes, network reads
            default_value: Some(ObjectValue::Unsigned8(0)),
            value_range: None,
            pdo_mapping: Some(PdoMapping::Optional),
        },
    );
    od.insert(
        IDX_ANALOG_INPUTS,
        ObjectEntry {
            object: Object::Array(vec![
                ObjectValue::Unsigned16(0), // Sub-index 1
                ObjectValue::Unsigned16(0), // Sub-index 2
                ObjectValue::Unsigned16(0), // Sub-index 3
                ObjectValue::Unsigned16(0), // Sub-index 4
            ]),
            name: "Analog_Inputs_4x16bit",
            category: Category::Mandatory,
            access: None,
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );

    // Outputs (data sink on CN)
    od.insert(
        IDX_DIGITAL_OUTPUTS,
        ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned8(0)),
            name: "Digital_Outputs_8bit",
            category: Category::Mandatory,
            access: Some(AccessType::ReadWrite), // Network writes, application reads
            default_value: Some(ObjectValue::Unsigned8(0)),
            value_range: None,
            pdo_mapping: Some(PdoMapping::Optional),
        },
    );
    od.insert(
        IDX_ANALOG_OUTPUTS,
        ObjectEntry {
            object: Object::Array(vec![
                ObjectValue::Unsigned16(0),
                ObjectValue::Unsigned16(0),
                ObjectValue::Unsigned16(0),
                ObjectValue::Unsigned16(0),
            ]),
            name: "Analog_Outputs_4x16bit",
            category: Category::Mandatory,
            access: None,
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );

    // --- PDO Configuration ---

    // TPDO1: Transmit inputs from CN to MN (in PRes)
    let tpdo1_map_di = PdoMappingEntry {
        index: IDX_DIGITAL_INPUTS,
        sub_index: 0,
        offset_bits: 0,
        length_bits: 8,
    };
    let tpdo1_map_ai1 = PdoMappingEntry {
        index: IDX_ANALOG_INPUTS,
        sub_index: 1,
        offset_bits: 8,
        length_bits: 16,
    };
    od.insert(
        0x1A00, // TPDO1 Mapping
        ObjectEntry {
            object: Object::Array(vec![
                // Sub-index 0 (count) is handled by the OD logic, not part of the Vec.
                ObjectValue::Unsigned64(tpdo1_map_di.to_u64()),
                ObjectValue::Unsigned64(tpdo1_map_ai1.to_u64()),
            ]),
            name: "PDO_TxMappParam_00h_AU64",
            category: Category::Mandatory,
            access: Some(AccessType::ReadWriteStore),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );
    // Manually write the *active* number of entries to sub-index 0
    od.write(0x1A00, 0, ObjectValue::Unsigned8(2)).unwrap();

    // RPDO1: Receive outputs from MN to CN (in PReq)
    let rpdo1_map_do = PdoMappingEntry {
        index: IDX_DIGITAL_OUTPUTS,
        sub_index: 0,
        offset_bits: 0,
        length_bits: 8,
    };
    let rpdo1_map_ao1 = PdoMappingEntry {
        index: IDX_ANALOG_OUTPUTS,
        sub_index: 1,
        offset_bits: 8,
        length_bits: 16,
    };
    od.insert(
        0x1600, // RPDO1 Mapping
        ObjectEntry {
            object: Object::Array(vec![
                // Sub-index 0 (count) is handled by the OD logic, not part of the Vec.
                ObjectValue::Unsigned64(rpdo1_map_do.to_u64()),
                ObjectValue::Unsigned64(rpdo1_map_ao1.to_u64()),
            ]),
            name: "PDO_RxMappParam_00h_AU64",
            category: Category::Mandatory,
            access: Some(AccessType::ReadWriteStore),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );
    // Manually write the *active* number of entries to sub-index 0
    od.write(0x1600, 0, ObjectValue::Unsigned8(2)).unwrap();

    od
}

/// Creates the Object Dictionary for the Managing Node.
fn get_mn_od(cn_mac: MacAddress) -> ObjectDictionary<'static> {
    let mut od = ObjectDictionary::new(None);

    // --- Mandatory MN Communication Objects ---
    add_mandatory_mn_objects(&mut od);

    // This object maps Node IDs to MAC addresses.
    // We use a manufacturer-specific index (0x2100) for this.
    let mut mac_map_entries = Vec::with_capacity(254);
    for i in 1..=254 {
        if i == CN_NODE_ID as usize {
            mac_map_entries.push(ObjectValue::OctetString(cn_mac.0.to_vec()));
        } else {
            mac_map_entries.push(ObjectValue::OctetString(vec![0; 6])); // Empty entry
        }
    }
    od.insert(
        0x2100, // Using 0x2100 (manufacturer-specific) for the MAC map
        ObjectEntry {
            object: Object::Array(mac_map_entries),
            name: "MAN_CNMacAddressList_AOS", // Manufacturer-specific Array of OctetString
            category: Category::Optional,     // Manufacturer-specific objects are not mandatory
            access: Some(AccessType::ReadWrite),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );

    // --- Application Data Objects (to store data from CN) ---
    // Mirror the CN's structure for clarity.
    od.insert(
        IDX_DIGITAL_INPUTS,
        ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned8(0)),
            name: "Digital_Inputs_CN42",
            category: Category::Mandatory,
            access: Some(AccessType::ReadOnly),
            default_value: Some(ObjectValue::Unsigned8(0)),
            value_range: None,
            pdo_mapping: None,
        },
    );
    od.insert(
        IDX_ANALOG_INPUTS,
        ObjectEntry {
            object: Object::Array(vec![
                ObjectValue::Unsigned16(0),
                ObjectValue::Unsigned16(0),
                ObjectValue::Unsigned16(0),
                ObjectValue::Unsigned16(0),
            ]),
            name: "Analog_Inputs_CN42",
            category: Category::Mandatory,
            access: None,
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );
    od.insert(
        IDX_DIGITAL_OUTPUTS,
        ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned8(0)),
            name: "Digital_Outputs_CN42",
            category: Category::Mandatory,
            access: Some(AccessType::ReadWrite),
            default_value: Some(ObjectValue::Unsigned8(0)),
            value_range: None,
            pdo_mapping: None,
        },
    );
    od.insert(
        IDX_ANALOG_OUTPUTS,
        ObjectEntry {
            object: Object::Array(vec![
                ObjectValue::Unsigned16(0),
                ObjectValue::Unsigned16(0),
                ObjectValue::Unsigned16(0),
                ObjectValue::Unsigned16(0),
            ]),
            name: "Analog_Outputs_CN42",
            category: Category::Mandatory,
            access: None,
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );

    // --- PDO Configuration for MN ---

    // RPDO to receive data from CN 42's PRes
    od.insert(
        0x1401, // Use RPDO channel 1 for CN 42
        ObjectEntry {
            object: Object::Record(vec![
                ObjectValue::Unsigned8(CN_NODE_ID), // Listen to this Node ID
                ObjectValue::Unsigned8(0),          // Mapping version
            ]),
            name: "PDO_RxCommParam_01h_REC",
            category: Category::Mandatory,
            access: Some(AccessType::ReadWriteStore),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );
    // The RPDO mapping must match the CN's TPDO mapping
    let rpdo1_map_di = PdoMappingEntry {
        index: IDX_DIGITAL_INPUTS,
        sub_index: 0,
        offset_bits: 0,
        length_bits: 8,
    };
    let rpdo1_map_ai1 = PdoMappingEntry {
        index: IDX_ANALOG_INPUTS,
        sub_index: 1,
        offset_bits: 8,
        length_bits: 16,
    };
    od.insert(
        0x1601, // RPDO Mapping for channel 1
        ObjectEntry {
            object: Object::Array(vec![
                ObjectValue::Unsigned64(rpdo1_map_di.to_u64()),
                ObjectValue::Unsigned64(rpdo1_map_ai1.to_u64()),
            ]),
            name: "PDO_RxMappParam_01h_AU64",
            category: Category::Mandatory,
            access: Some(AccessType::ReadWriteStore),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );
    // Manually write the *active* number of entries to sub-index 0
    od.write(0x1601, 0, ObjectValue::Unsigned8(2)).unwrap();

    // TPDO to send data to CN 42 via PReq
    od.insert(
        0x1801, // Use TPDO channel 1 for CN 42
        ObjectEntry {
            object: Object::Record(vec![
                ObjectValue::Unsigned8(CN_NODE_ID), // Target this Node ID
                ObjectValue::Unsigned8(0),          // Mapping version
            ]),
            name: "PDO_TxCommParam_01h_REC",
            category: Category::Mandatory,
            access: Some(AccessType::ReadWriteStore),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );
    // The TPDO mapping must match the CN's RPDO mapping
    let tpdo1_map_do = PdoMappingEntry {
        index: IDX_DIGITAL_OUTPUTS,
        sub_index: 0,
        offset_bits: 0,
        length_bits: 8,
    };
    let tpdo1_map_ao1 = PdoMappingEntry {
        index: IDX_ANALOG_OUTPUTS,
        sub_index: 1,
        offset_bits: 8,
        length_bits: 16,
    };
    od.insert(
        0x1A01, // TPDO Mapping for channel 1
        ObjectEntry {
            object: Object::Array(vec![
                ObjectValue::Unsigned64(tpdo1_map_do.to_u64()),
                ObjectValue::Unsigned64(tpdo1_map_ao1.to_u64()),
            ]),
            name: "PDO_TxMappParam_01h_AU64",
            category: Category::Mandatory,
            access: Some(AccessType::ReadWriteStore),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );
    // Manually write the *active* number of entries to sub-index 0
    od.write(0x1A01, 0, ObjectValue::Unsigned8(2)).unwrap();

    od
}

/// The main loop for the Controlled Node application.
fn run_cn_logic(interface_name: &str) {
    info!("[CN] Starting up as I/O Module (Node ID {}).", CN_NODE_ID);
    let od = get_cn_od(CN_NODE_ID);

    let (mut interface, mut node) =
        setup_cn_node(interface_name, CN_NODE_ID, od).expect("Failed to setup CN");

    let mut eth_buffer = [0u8; 1518];
    // The `sdo-udp` feature is enabled for this example
    let mut udp_buffer = [0u8; 1500];
    let start_time = Instant::now();
    let mut digital_input_counter: u8 = 0;

    loop {
        let current_time_us = start_time.elapsed().as_micros() as u64;

        // --- Application Logic: Simulate Hardware I/O ---
        // 1. Read simulated hardware inputs and write to OD
        digital_input_counter = digital_input_counter.wrapping_add(1);
        node.context
            .core
            .od
            .write(
                IDX_DIGITAL_INPUTS,
                0,
                ObjectValue::Unsigned8(digital_input_counter),
            )
            .unwrap();

        // 2. Read outputs from OD (written by MN) and "write" to simulated hardware
        if let Some(do_val) = node.context.core.od.read_u8(IDX_DIGITAL_OUTPUTS, 0) {
            if do_val != 0 {
                info!("[CN] Digital outputs received from MN: {:#04x}", do_val);
            }
        }

        // --- Network Stack Logic ---
        // 1. Poll for Ethernet frames
        let eth_slice = match interface.receive_frame(&mut eth_buffer) {
            Ok(bytes) if bytes > 0 => Some(&eth_buffer[..bytes]),
            _ => None,
        };

        // 2. Poll for UDP datagrams
        let udp_info: Option<(&[u8], IpAddress, u16)> = match interface.receive_udp(&mut udp_buffer)
        {
            Ok(Some((size, ip, port))) => Some((&udp_buffer[..size], ip, port)),
            _ => None,
        };

        // 3. Call the single run_cycle function
        let action = node.run_cycle(eth_slice, udp_info, current_time_us);

        // 4. Execute the returned action
        match action {
            NodeAction::SendFrame(frame) => {
                if let Err(e) = interface.send_frame(&frame) {
                    error!("[CN] Failed to send frame: {:?}", e);
                }
            }
            NodeAction::SendUdp {
                dest_ip,
                dest_port,
                data,
            } => {
                if let Err(e) = interface.send_udp(dest_ip, dest_port, &data) {
                    error!("[CN] Failed to send UDP: {:?}", e);
                }
            }
            NodeAction::NoAction => {
                // Nothing to do
            }
        }

        // Small sleep to prevent busy-looping if receive_frame returns immediately
        thread::sleep(Duration::from_micros(100));
    }
}

/// The main loop for the Managing Node application.
fn run_mn_logic(interface_name: &str, cn_mac: MacAddress) {
    info!("[MN] Starting up as Managing Node.");
    let od = get_mn_od(cn_mac);

    let (mut interface, mut node) = setup_mn_node(interface_name, od).expect("Failed to setup MN");

    let start_time = Instant::now();
    let mut last_log_time = Instant::now();
    let mut eth_buffer = [0u8; 1518];
    // The `sdo-udp` feature is enabled for this example
    let mut udp_buffer = [0u8; 1500];

    loop {
        let current_time_us = start_time.elapsed().as_micros() as u64;

        // The MN loop is driven by `tick()`. We check for the next action time.
        if let Some(deadline) = node.next_action_time() {
            if current_time_us < deadline {
                let wait_time = (deadline - current_time_us).min(1000); // Wait at most 1ms
                thread::sleep(Duration::from_micros(wait_time));
                // Continue to poll interfaces even while waiting for tick
            }
        }

        // 1. Poll for Ethernet frames
        let eth_slice = match interface.receive_frame(&mut eth_buffer) {
            Ok(bytes) if bytes > 0 => Some(&eth_buffer[..bytes]),
            _ => None,
        };

        // 2. Poll for UDP datagrams
        let udp_info: Option<(&[u8], IpAddress, u16)> = match interface.receive_udp(&mut udp_buffer)
        {
            Ok(Some((size, ip, port))) => Some((&udp_buffer[..size], ip, port)),
            _ => None,
        };

        // 3. Call the single run_cycle function
        let action = node.run_cycle(eth_slice, udp_info, current_time_us);

        // 4. Execute the returned action
        match action {
            NodeAction::SendFrame(frame) => {
                if let Err(e) = interface.send_frame(&frame) {
                    error!("[MN] Failed to send frame: {:?}", e);
                }
            }
            NodeAction::SendUdp {
                dest_ip,
                dest_port,
                data,
            } => {
                if let Err(e) = interface.send_udp(dest_ip, dest_port, &data) {
                    error!("[MN] Failed to send UDP: {:?}", e);
                }
            }
            NodeAction::NoAction => {
                // Nothing to do
            }
        }

        // --- Application Logic: Mirror Inputs to Outputs ---
        if node.nmt_state() == NmtState::NmtOperational {
            if let Some(di_val) = node.context.core.od.read_u8(IDX_DIGITAL_INPUTS, 0) {
                // Mirror inputs to outputs
                node.context
                    .core
                    .od
                    .write(IDX_DIGITAL_OUTPUTS, 0, ObjectValue::Unsigned8(di_val))
                    .unwrap();

                if last_log_time.elapsed() > Duration::from_secs(1) {
                    info!(
                        "[MN] Received Digital Inputs from CN {}: {:#04x}",
                        CN_NODE_ID, di_val
                    );
                    last_log_time = Instant::now();
                }
            }
        }
    }
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let role = env::var("POWERLINK_ROLE").unwrap_or_else(|_| "MN".to_string());
    // Get the CN's MAC address from an environment variable for the MN to use.
    // This is a bit of a cheat for a real-world discovery, but necessary for this example.
    let cn_mac_str = env::var("CN_MAC_ADDRESS").unwrap_or_else(|_| "02:00:00:00:00:42".to_string());
    let cn_mac_bytes: Vec<u8> = cn_mac_str
        .split(':')
        .map(|s| u8::from_str_radix(s, 16).unwrap())
        .collect();
    let cn_mac: [u8; 6] = cn_mac_bytes
        .try_into()
        .unwrap_or([0x02, 0x00, 0x00, 0x00, 0x00, 0x42]);

    if role == "CN" {
        run_cn_logic("eth0");
    } else {
        run_mn_logic("eth0", cn_mac.into());
    }
}

// --- Setup Helpers ---

// Add mandatory objects required for a basic CN
fn add_mandatory_cn_objects(od: &mut ObjectDictionary, node_id: u8) {
    od.insert(
        0x1000, // NMT_DeviceType_U32
        ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned32(0x000F0191)),
            name: "NMT_DeviceType_U32",
            category: Category::Mandatory,
            access: Some(AccessType::Constant),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );
    od.insert(
        0x1018, // NMT_IdentityObject_REC (already added in get_cn_od, but good to ensure)
        ObjectEntry {
            object: Object::Record(vec![
                ObjectValue::Unsigned32(0x12345678), // VendorId
                ObjectValue::Unsigned32(0x00000002), // ProductCode for IO Module
                ObjectValue::Unsigned32(0x00010000), // RevisionNo
                ObjectValue::Unsigned32(0x98765432), // SerialNo
            ]),
            name: "NMT_IdentityObject_REC",
            category: Category::Mandatory,
            access: None,
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );
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
    od.insert(
        0x1F99, // NMT_CNBasicEthernetTimeout_U32
        ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned32(5_000_000)), // 5 seconds
            name: "NMT_CNBasicEthernetTimeout_U32",
            category: Category::Mandatory,
            access: Some(AccessType::ReadWriteStore),
            default_value: Some(ObjectValue::Unsigned32(5_000_000)),
            value_range: None,
            pdo_mapping: None,
        },
    );
    od.insert(
        0x1006, // NMT_CycleLen_U32
        ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned32(CYCLE_TIME_US)),
            name: "NMT_CycleLen_U32",
            category: Category::Mandatory,
            access: Some(AccessType::ReadWriteStore),
            default_value: Some(ObjectValue::Unsigned32(CYCLE_TIME_US)),
            value_range: None,
            pdo_mapping: None,
        },
    );
    od.insert(
        0x1C14, // DLL_CNLossOfSocTolerance_U32
        ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned32(1_000_000)), // 1ms
            name: "DLL_CNLossOfSocTolerance_U32",
            category: Category::Mandatory,
            access: Some(AccessType::ReadWriteStore),
            default_value: Some(ObjectValue::Unsigned32(1_000_000)),
            value_range: None,
            pdo_mapping: None,
        },
    );
    // Add 0x1F8C for NMT state
    od.insert(
        0x1F8C, // NMT_CurrNMTState_U8
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
    // Add 0x1F98 for CycleTiming
    od.insert(
        0x1F98, // NMT_CycleTiming_REC (needed for PresActPayloadLimit)
        ObjectEntry {
            object: Object::Record(vec![
                ObjectValue::Unsigned16(1490),  // 1: IsochrTxMaxPayload_U16
                ObjectValue::Unsigned16(1490),  // 2: IsochrRxMaxPayload_U16
                ObjectValue::Unsigned32(10000), // 3: PresMaxLatency_U32 (10 us)
                ObjectValue::Unsigned16(36),    // 4: PreqActPayloadLimit_U16 (36 bytes default)
                ObjectValue::Unsigned16(36),    // 5: PresActPayloadLimit_U16 (36 bytes default)
                ObjectValue::Unsigned32(20000), // 6: AsndMaxLatency_U32 (20 us)
                ObjectValue::Unsigned8(0),      // 7: MultiplCycleCnt_U8
                ObjectValue::Unsigned16(300),   // 8: AsyncMTU_U16
                ObjectValue::Unsigned16(2),     // 9: Prescaler_U16
            ]),
            name: "NMT_CycleTiming_REC",
            category: Category::Mandatory,
            access: Some(AccessType::ReadWriteStore),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );
}

// Add mandatory objects required for a basic MN
fn add_mandatory_mn_objects(od: &mut ObjectDictionary) {
    od.insert(
        0x1000, // NMT_DeviceType_U32
        ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned32(0x000F0191)), // Generic Device
            name: "NMT_DeviceType_U32",
            category: Category::Mandatory,
            access: Some(AccessType::Constant),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );
    od.insert(
        0x1018, // Identity Object
        ObjectEntry {
            object: Object::Record(vec![
                ObjectValue::Unsigned32(0x12345678), // VendorId
                ObjectValue::Unsigned32(0x00000001), // ProductCode for MN
                ObjectValue::Unsigned32(0x00010000), // RevisionNo
                ObjectValue::Unsigned32(0x12345678), // SerialNo
            ]),
            name: "NMT_IdentityObject_REC",
            category: Category::Mandatory,
            access: None,
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );
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
        0x1006, // NMT_CycleLen_U32
        ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned32(CYCLE_TIME_US)),
            name: "NMT_CycleLen_U32",
            category: Category::Mandatory,
            access: Some(AccessType::ReadWriteStore),
            default_value: Some(ObjectValue::Unsigned32(CYCLE_TIME_US)),
            value_range: None,
            pdo_mapping: None,
        },
    );

    let mut node_assignment = vec![]; // Sub-index 0 is handled by OD logic
    for i in 1..=254 {
        if i == CN_NODE_ID as usize {
            // Bit 0: Node exists, Bit 3: Node is mandatory
            node_assignment.push(ObjectValue::Unsigned32(1 | (1 << 3)));
        } else {
            node_assignment.push(ObjectValue::Unsigned32(0));
        }
    }
    od.insert(
        0x1F81, // NMT_NodeAssignment_AU32
        ObjectEntry {
            object: Object::Array(node_assignment),
            name: "NMT_NodeAssignment_AU32",
            category: Category::Mandatory,
            access: Some(AccessType::ReadWriteStore),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );
    // Manually set sub-index 0 (count)
    od.write(0x1F81, 0, ObjectValue::Unsigned8(254)).unwrap();

    od.insert(
        0x1F89, // NMT_BootTime_REC
        ObjectEntry {
            object: Object::Record(vec![
                ObjectValue::Unsigned32(1_000_000), // 1: MNWaitNotAct_U32 (1 sec)
                ObjectValue::Unsigned32(500_000),   // 2: MNTimeoutPreOp1_U32 (500 ms)
                // Add missing sub-indices up to 9
                ObjectValue::Unsigned32(500_000), // 3: MNWaitPreOp1_U32
                ObjectValue::Unsigned32(500_000), // 4: MNTimeoutPreOp2_U32
                ObjectValue::Unsigned32(500_000), // 5: MNTimeoutReadyToOp_U32
                ObjectValue::Unsigned32(500_000), // 6: MNIdentificationTimeout_U32
                ObjectValue::Unsigned32(500_000), // 7: MNSoftwareTimeout_U32
                ObjectValue::Unsigned32(500_000), // 8: MNConfigurationTimeout_U32
                ObjectValue::Unsigned32(500_000), // 9: MNStartCNTimeout_U32
            ]),
            name: "NMT_BootTime_REC",
            category: Category::Mandatory,
            access: Some(AccessType::ReadWriteStore),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );

    od.insert(
        0x1F80, // NMT_StartUp_U32
        ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned32(0)), // Auto-boot
            name: "NMT_StartUp_U32",
            category: Category::Mandatory,
            access: Some(AccessType::ReadWriteStore),
            default_value: Some(ObjectValue::Unsigned32(0)),
            value_range: None,
            pdo_mapping: None,
        },
    );

    od.insert(
        0x1F98, // NMT_CycleTiming_REC
        ObjectEntry {
            object: Object::Record(vec![
                ObjectValue::Unsigned16(1490),  // 1: IsochrTxMaxPayload_U16
                ObjectValue::Unsigned16(1490),  // 2: IsochrRxMaxPayload_U16
                ObjectValue::Unsigned32(10000), // 3: PresMaxLatency_U32 (10 us)
                ObjectValue::Unsigned16(100),   // 4: PreqActPayloadLimit_U16 (not used by MN)
                ObjectValue::Unsigned16(100),   // 5: PresActPayloadLimit_U16
                ObjectValue::Unsigned32(20000), // 6: AsndMaxLatency_U32 (20 us)
                ObjectValue::Unsigned8(0),      // 7: MultiplCycleCnt_U8
                ObjectValue::Unsigned16(300),   // 8: AsyncMTU_U16
                ObjectValue::Unsigned16(2),     // 9: Prescaler_U16
            ]),
            name: "NMT_CycleTiming_REC",
            category: Category::Mandatory,
            access: Some(AccessType::ReadWriteStore),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );
    // Add 0x1F8C for NMT state
    od.insert(
        0x1F8C, // NMT_CurrNMTState_U8
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
    // Add 0x1F8B for PReq Payload Limits
    let mut preq_limits = vec![]; // Sub-index 0 is handled by OD logic
    for i in 1..=254 {
        if i == CN_NODE_ID as usize {
            preq_limits.push(ObjectValue::Unsigned16(36)); // 36 bytes for our CN
        } else {
            preq_limits.push(ObjectValue::Unsigned16(36)); // 36 bytes default
        }
    }
    od.insert(
        0x1F8B, // NMT_MNPReqPayloadLimitList_AU16
        ObjectEntry {
            object: Object::Array(preq_limits),
            name: "NMT_MNPReqPayloadLimitList_AU16",
            category: Category::Mandatory,
            access: Some(AccessType::ReadWriteStore),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );
    // Manually set sub-index 0 (count)
    od.write(0x1F8B, 0, ObjectValue::Unsigned8(254)).unwrap();

    // Add 0x1F8D for PRes Payload Limits
    let mut pres_limits = vec![]; // Sub-index 0 is handled by OD logic
    for i in 1..=254 {
        if i == CN_NODE_ID as usize {
            pres_limits.push(ObjectValue::Unsigned16(36)); // 36 bytes for our CN
        } else {
            pres_limits.push(ObjectValue::Unsigned16(36)); // 36 bytes default
        }
    }
    od.insert(
        0x1F8D, // NMT_PresPayloadLimitList_AU16
        ObjectEntry {
            object: Object::Array(pres_limits),
            name: "NMT_PresPayloadLimitList_AU16",
            category: Category::Mandatory,
            access: Some(AccessType::ReadWriteStore),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );
    // Manually set sub-index 0 (count)
    od.write(0x1F8D, 0, ObjectValue::Unsigned8(254)).unwrap();

    // Add 0x1F92 for PRes Timeouts
    let mut pres_timeouts = vec![]; // Sub-index 0 is handled by OD logic
    for _ in 1..=254 {
        pres_timeouts.push(ObjectValue::Unsigned32(100_000)); // 100us timeout
    }
    od.insert(
        0x1F92, // NMT_MNCNPResTimeout_AU32
        ObjectEntry {
            object: Object::Array(pres_timeouts),
            name: "NMT_MNCNPResTimeout_AU32",
            category: Category::Mandatory,
            access: Some(AccessType::ReadWriteStore),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );
    // Manually set sub-index 0 (count)
    od.write(0x1F92, 0, ObjectValue::Unsigned8(254)).unwrap();
}

// Corrected function signatures with lifetimes
fn setup_cn_node<'a>(
    interface_name: &str,
    node_id: u8,
    od: ObjectDictionary<'a>,
) -> Result<(LinuxPnetInterface, ControlledNode<'a>), PowerlinkError> {
    let mut interface = match LinuxPnetInterface::new(interface_name, node_id) {
        Ok(iface) => iface,
        Err(e) => {
            error!("[CN] Failed to create network interface: {:?}", e);
            return Err(PowerlinkError::IoError);
        }
    };
    interface.set_read_timeout(Duration::from_millis(10))?; // Set a default timeout
    let mac = interface.local_mac_address();
    let node = ControlledNode::new(od, mac.into())?;
    Ok((interface, node))
}

fn setup_mn_node<'a>(
    interface_name: &str,
    od: ObjectDictionary<'a>,
) -> Result<(LinuxPnetInterface, ManagingNode<'a>), PowerlinkError> {
    let mut interface = match LinuxPnetInterface::new(interface_name, C_ADR_MN_DEF_NODE_ID) {
        Ok(iface) => iface,
        Err(e) => {
            error!("[MN] Failed to create network interface: {:?}", e);
            return Err(PowerlinkError::IoError);
        }
    };
    interface.set_read_timeout(Duration::from_micros(100))?; // Short timeout for non-blocking feel
    let mac = interface.local_mac_address();
    let node = ManagingNode::new(od, mac.into())?;
    Ok((interface, node))
}
