// tests/boot_up_test.rs

// Import the shared simulator module.
// Rust looks for `tests/simulator/mod.rs` when we declare `mod simulator;` here.
#[cfg(feature = "std")]
mod simulator;

#[cfg(feature = "std")]
mod tests {
    // Use the local simulator module we declared above
    use super::simulator::{NodeHarness, SimulatedInterface, VirtualNetwork};
    
    use powerlink_rs::{
        ControlledNode, Node, NodeId, 
        ObjectDictionaryStorage, PowerlinkError,
    };
    use powerlink_rs::frame::basic::MacAddress;
    use powerlink_rs::node::ManagingNode;

    use powerlink_rs::nmt::states::NmtState;
    use powerlink_rs::od::{ObjectDictionary, ObjectEntry, ObjectValue, Category, AccessType}; 
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::collections::BTreeMap;
    use std::fs::File;
    use std::fs;

    // --- Mock Storage for OD ---
    struct MockStorage;
    impl ObjectDictionaryStorage for MockStorage {
        fn load(&mut self) -> Result<BTreeMap<(u16, u8), ObjectValue>, PowerlinkError> { Ok(BTreeMap::new()) }
        fn save(&mut self, _p: &BTreeMap<(u16, u8), ObjectValue>) -> Result<(), PowerlinkError> { Ok(()) }
        fn clear(&mut self) -> Result<(), PowerlinkError> { Ok(()) }
        fn restore_defaults_requested(&self) -> bool { false }
        fn request_restore_defaults(&mut self) -> Result<(), PowerlinkError> { Ok(()) }
        fn clear_restore_defaults_flag(&mut self) -> Result<(), PowerlinkError> { Ok(()) }
    }

    // Mock Time Provider for the Test
    struct MockTimeProvider;
    impl powerlink_rs::hal::TimeProvider for MockTimeProvider {
        fn now_monotonic_us(&self) -> u64 { 0 }
        fn now_net_time(&self) -> powerlink_rs::common::NetTime { powerlink_rs::common::NetTime::default() }
    }

    // Helper to create a default ObjectEntry since the trait impl isn't visible here
    fn default_object_entry(value: ObjectValue) -> ObjectEntry {
        ObjectEntry {
            object: powerlink_rs::od::Object::Variable(value),
            name: "TestObject",
            category: Category::Optional,
            access: None,
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        }
    }

    // Updated signature to accept TimeProvider reference
    fn create_cn<'a>(node_id: u8, time_provider: &'a MockTimeProvider) -> NodeHarness<ControlledNode<'a>> {
        let mac = MacAddress([0x02, 0x00, 0x00, 0x00, 0x00, node_id]);
        
        // Setup minimal OD
        let mut od = powerlink_rs::od::utils::new_cn_default(NodeId(node_id)).unwrap();
        // Required by IdentResponse
        od.insert(0x1000, default_object_entry(ObjectValue::Unsigned32(0x12345678)));
        
        // Pass time_provider to ControlledNode::new
        let node = ControlledNode::new(od, mac, time_provider).unwrap();
        let interface = Rc::new(RefCell::new(SimulatedInterface::new(node_id, mac.0)));
        
        NodeHarness::new(node, interface, NodeId(node_id))
    }

    // Updated signature to accept TimeProvider reference
    fn create_mn<'a>(time_provider: &'a MockTimeProvider) -> NodeHarness<ManagingNode<'a>> {
        let node_id = 240;
        let mac = MacAddress([0x02, 0x00, 0x00, 0x00, 0x00, 0xF0]);
        
        // Setup minimal OD for MN
        let mut od = powerlink_rs::od::utils::new_mn_default(NodeId(node_id)).unwrap();
        
        // Configure Node 1 as mandatory
        // 0x1F81 sub 1: NodeAssignment for Node 1
        // Bits: 0(Exists)=1, 1(IsCN)=1, 3(Mandatory)=1, 8(Isochr)=0(default) -> 0b1011 = 0xB
        od.write(0x1F81, 1, ObjectValue::Unsigned32(0xB)).unwrap();
        
        // Configure Expected Ident for Node 1 (match CN's default)
        od.write(0x1F84, 1, ObjectValue::Unsigned32(0)).unwrap(); // DeviceType (0=don't check)
        
        let node = ManagingNode::new(od, mac, None, time_provider).unwrap();
        let interface = Rc::new(RefCell::new(SimulatedInterface::new(node_id, mac.0)));
        
        NodeHarness::new(node, interface, NodeId(node_id))
    }

    #[test]
    fn test_boot_up_sequence() {
        // 1. Initialize File Logger
        // Create log folder
        let _ = fs::create_dir("tests/boot_up_test");
        // File::create truncates the file if it exists, satisfying the overwrite requirement.
        let log_file = File::create("tests/boot_up_test/test_boot_up_sequence.log").expect("Could not create log file");
        
        let _ = env_logger::Builder::new()
            .target(env_logger::Target::Pipe(Box::new(log_file)))
            .filter_level(log::LevelFilter::Trace)
            .format_timestamp_micros() // High precision timing is useful for PLK
            .try_init();

        // 2. Initialize Resources
        let mut network = VirtualNetwork::new();
        network.register_node(1);
        network.register_node(240);

        // Instantiate TimeProvider once here
        let time_provider = MockTimeProvider;

        // Pass references to helpers
        let mut cn = create_cn(1, &time_provider);
        let mut mn = create_mn(&time_provider);

        // Run simulation loop
        // We tick in 1ms increments (1000us)
        let dt = 1000; 
        let max_time = 5_000_000; // 5 seconds max
        
        let mut mn_reached_operational = false;
        let mut cn_reached_operational = false;

        while network.current_time() < max_time {
            // Run cycles
            mn.run_cycle(&mut network);
            cn.run_cycle(&mut network);
            
            // Check states
            if mn.node.nmt_state() == NmtState::NmtOperational {
                mn_reached_operational = true;
            }
            if cn.node.nmt_state() == NmtState::NmtOperational {
                cn_reached_operational = true;
            }

            if mn_reached_operational && cn_reached_operational {
                break;
            }

            network.tick(dt);
        }
        
        // Dump history regardless of success/failure to assist debugging
        if let Err(e) = network.dump_history_to_file("tests/boot_up_test/test_boot_up_sequence_packets.log") {
            println!("Warning: Failed to dump packet history: {}", e);
        }

        // Dump State logs
        if let Err(e) = mn.dump_state_log("tests/boot_up_test/mn_states.log") {
             println!("Warning: Failed to dump MN state log: {}", e);
        }
        if let Err(e) = cn.dump_state_log("tests/boot_up_test/cn_states.log") {
             println!("Warning: Failed to dump CN state log: {}", e);
        }

        assert!(mn_reached_operational, "MN did not reach Operational state. Current: {:?}", mn.node.nmt_state());
        assert!(cn_reached_operational, "CN did not reach Operational state. Current: {:?}", cn.node.nmt_state());
    }
}