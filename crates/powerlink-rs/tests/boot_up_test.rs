// crates/powerlink-rs/tests/boot_up_test.rs

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
    use std::fs::File; // <-- Added for file I/O

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

    fn create_cn(node_id: u8) -> NodeHarness<ControlledNode<'static>> {
        let mac = MacAddress([0x02, 0x00, 0x00, 0x00, 0x00, node_id]);
        
        // Setup minimal OD
        let mut od = powerlink_rs::od::utils::new_cn_default(NodeId(node_id)).unwrap();
        // Required by IdentResponse
        od.insert(0x1000, default_object_entry(ObjectValue::Unsigned32(0x12345678)));
        
        let node = ControlledNode::new(od, mac).unwrap();
        let interface = Rc::new(RefCell::new(SimulatedInterface::new(node_id, mac.0)));
        
        NodeHarness::new(node, interface, NodeId(node_id))
    }

    fn create_mn() -> NodeHarness<ManagingNode<'static>> {
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
        
        let node = ManagingNode::new(od, mac, None).unwrap();
        let interface = Rc::new(RefCell::new(SimulatedInterface::new(node_id, mac.0)));
        
        NodeHarness::new(node, interface, NodeId(node_id))
    }

    #[test]
    fn test_boot_up_sequence() {
        // 1. Initialize File Logger
        // File::create truncates the file if it exists, satisfying the overwrite requirement.
        let log_file = File::create("tests/test_boot_up_sequence.log").expect("Could not create log file");
        
        let _ = env_logger::Builder::new()
            .target(env_logger::Target::Pipe(Box::new(log_file)))
            .filter_level(log::LevelFilter::Trace)
            .format_timestamp_micros() // High precision timing is useful for PLK
            .try_init();

        let mut network = VirtualNetwork::new();
        network.register_node(1);
        network.register_node(240);

        let mut cn = create_cn(1);
        let mut mn = create_mn();

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

        assert!(mn_reached_operational, "MN did not reach Operational state. Current: {:?}", mn.node.nmt_state());
        assert!(cn_reached_operational, "CN did not reach Operational state. Current: {:?}", cn.node.nmt_state());
    }
}