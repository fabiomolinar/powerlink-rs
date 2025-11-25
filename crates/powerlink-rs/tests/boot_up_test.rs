// tests/boot_up_test.rs

#[cfg(feature = "std")]
mod simulator;

#[cfg(feature = "std")]
mod tests {
    // Use the local simulator module we declared above
    use super::simulator::{NodeHarness, SimulatedInterface, VirtualNetwork, SimulatedTimeProvider};
    
    use powerlink_rs::{
        ControlledNode, Node, NodeId, 
        ObjectDictionaryStorage, PowerlinkError,
        hal::TimeProvider,
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

    fn create_cn(node_id: u8, time_provider: &dyn TimeProvider) -> NodeHarness<ControlledNode<'_>> {
        let mac = MacAddress([0x02, 0x00, 0x00, 0x00, 0x00, node_id]);
        
        let mut od = powerlink_rs::od::utils::new_cn_default(NodeId(node_id)).unwrap();
        od.insert(0x1000, default_object_entry(ObjectValue::Unsigned32(0x12345678)));
        
        let node = ControlledNode::new(od, mac, time_provider).unwrap();
        let interface = Rc::new(RefCell::new(SimulatedInterface::new(node_id, mac.0)));
        
        NodeHarness::new(node, interface, NodeId(node_id))
    }

    fn create_mn(time_provider: &dyn TimeProvider) -> NodeHarness<ManagingNode<'_>> {
        let node_id = 240;
        let mac = MacAddress([0x02, 0x00, 0x00, 0x00, 0x00, 0xF0]);
        
        let mut od = powerlink_rs::od::utils::new_mn_default(NodeId(node_id)).unwrap();
        od.write(0x1F81, 1, ObjectValue::Unsigned32(0xB)).unwrap();
        od.write(0x1F84, 1, ObjectValue::Unsigned32(0)).unwrap(); 
        
        let node = ManagingNode::new(od, mac, None, time_provider).unwrap();
        let interface = Rc::new(RefCell::new(SimulatedInterface::new(node_id, mac.0)));
        
        NodeHarness::new(node, interface, NodeId(node_id))
    }

    #[test]
    fn test_boot_up_sequence() {
        let _ = fs::create_dir("tests/boot_up_test");
        let log_file = File::create("tests/boot_up_test/test_boot_up_sequence.log").expect("Could not create log file");
        
        let _ = env_logger::Builder::new()
            .target(env_logger::Target::Pipe(Box::new(log_file)))
            .filter_level(log::LevelFilter::Trace)
            .format_timestamp_micros()
            .try_init();

        // Shared time source
        let shared_time = Rc::new(RefCell::new(0u64));
        
        // Network uses shared time
        let mut network = VirtualNetwork::new_with_shared_time(shared_time.clone());
        network.register_node(1);
        network.register_node(240);

        // Provider for nodes uses same shared time
        let time_provider = SimulatedTimeProvider::new(shared_time.clone());

        let mut cn = create_cn(1, &time_provider);
        let mut mn = create_mn(&time_provider);

        let dt = 1000; 
        let max_time = 5_000_000; 
        
        let mut mn_reached_operational = false;
        let mut cn_reached_operational = false;

        while network.current_time() < max_time {
            mn.run_cycle(&mut network);
            cn.run_cycle(&mut network);
            
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
        
        if let Err(e) = network.dump_history_to_file("tests/boot_up_test/test_boot_up_sequence_packets.log") {
            println!("Warning: Failed to dump packet history: {}", e);
        }

        assert!(mn_reached_operational, "MN did not reach Operational state. Current: {:?}", mn.node.nmt_state());
        assert!(cn_reached_operational, "CN did not reach Operational state. Current: {:?}", cn.node.nmt_state());
    }
}