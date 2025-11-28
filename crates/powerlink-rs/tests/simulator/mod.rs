// tests/simulator/mod.rs
pub mod interface;

use powerlink_rs::node::{Node, NodeAction};
use powerlink_rs::types::NodeId;
// Fix E0603: Re-export the interface so it's accessible to tests
pub use interface::SimulatedInterface; 
// Fix E0599: Import trait to use send_frame/receive_frame methods
use powerlink_rs::NetworkInterface; 
use powerlink_rs::frame::{deserialize_frame, PowerlinkFrame};

// Imports for State Inspection
use powerlink_rs::node::{ControlledNode, ManagingNode};
// use powerlink_rs::nmt::states::NmtState; // Removed unused import

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::fs::File;
use std::io::{Write, BufWriter};

/// Represents a packet in flight on the virtual network.
#[derive(Debug, Clone)]
pub struct Packet {
    pub data: Vec<u8>,
    pub src_node_id: u8, // 0 for unknown/external
    pub transmit_time_us: u64,
}

/// A virtual POWERLINK network that manages time and packet delivery.
pub struct VirtualNetwork {
    /// Current simulation time in microseconds.
    current_time_us: u64,
    /// Pending packets to be delivered to nodes.
    /// Map: NodeId (Destination) -> Queue of Packets
    inboxes: HashMap<u8, VecDeque<Packet>>,
    /// Trace of all packets sent on the network (for assertions).
    pub packet_history: Vec<Packet>,
}

impl VirtualNetwork {
    pub fn new() -> Self {
        Self {
            current_time_us: 0,
            inboxes: HashMap::new(),
            packet_history: Vec::new(),
        }
    }

    /// Advances simulation time.
    pub fn tick(&mut self, duration_us: u64) {
        self.current_time_us += duration_us;
    }

    pub fn current_time(&self) -> u64 {
        self.current_time_us
    }

    /// Simulates sending a frame from a source to a destination (or broadcast).
    /// This is called by the `SimulatedInterface` internals.
    pub fn transmit(&mut self, packet: Packet, dest_node_id: Option<u8>) {
        self.packet_history.push(packet.clone());

        if let Some(dest) = dest_node_id {
            // Unicast
            self.inboxes
                .entry(dest)
                .or_insert_with(VecDeque::new)
                .push_back(packet);
        } else {
            // Broadcast: Deliver to all known inboxes (simplification for now)
            for queue in self.inboxes.values_mut() {
                queue.push_back(packet.clone());
            }
        }
    }

    /// Retrieves the next packet for a specific node.
    pub fn receive(&mut self, node_id: u8) -> Option<Packet> {
        self.inboxes
            .entry(node_id)
            .or_insert_with(VecDeque::new)
            .pop_front()
    }
    
    /// Registers a node (creates an inbox)
    pub fn register_node(&mut self, node_id: u8) {
        self.inboxes.entry(node_id).or_insert_with(VecDeque::new);
    }

    /// Dumps the entire frame history to a log file.
    /// 
    /// format: [Timestamp_us] [NodeType NodeID] -> [FrameType]: Description
    pub fn dump_history_to_file(&self, path: &str) -> std::io::Result<()> {
        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);

        writeln!(writer, "--- Virtual Network Frame History ---")?;
        writeln!(writer, "Total Frames: {}", self.packet_history.len())?;
        writeln!(writer, "-------------------------------------")?;

        for packet in &self.packet_history {
            let node_type = if packet.src_node_id == 240 { "MN" } else { "CN" };
            let frame_info = match deserialize_frame(&packet.data) {
                Ok(frame) => match frame {
                    PowerlinkFrame::Soc(f) => format!("SoC (RelTime: {:?})", f.relative_time),
                    PowerlinkFrame::PReq(f) => format!("PReq (Dest: {})", f.destination.0),
                    PowerlinkFrame::PRes(f) => format!("PRes (Src: {})", f.source.0),
                    PowerlinkFrame::SoA(f) => format!("SoA (ReqSvc: {:?}, Target: {})", f.req_service_id, f.target_node_id.0),
                    PowerlinkFrame::ASnd(f) => format!("ASnd (Svc: {:?}, Dest: {})", f.service_id, f.destination.0),
                },
                Err(e) => format!("Invalid/Unknown Frame ({:?})", e),
            };

            writeln!(
                writer,
                "[{:08} us] [{} {:03}] -> {}",
                packet.transmit_time_us,
                node_type,
                packet.src_node_id,
                frame_info
            )?;
        }
        
        writeln!(writer, "-------------------------------------")?;
        Ok(())
    }
}

/// Trait to inspect internal state of a Node for debugging logs.
pub trait Inspectable {
    // Removed nmt_state() to resolve collision with Node::nmt_state()
    fn dll_state_str(&self) -> String;
}

impl<'a> Inspectable for ControlledNode<'a> {
    fn dll_state_str(&self) -> String {
        format!("{:?}", self.context.dll_state_machine.current_state())
    }
}

impl<'a> Inspectable for ManagingNode<'a> {
    fn dll_state_str(&self) -> String {
        format!("{:?} (Phase: {:?})", 
            self.context.dll_state_machine.current_state(),
            self.context.current_phase
        )
    }
}

/// Wraps a `Node` (MN or CN) and its `SimulatedInterface` for the test harness.
pub struct NodeHarness<N: Node + Inspectable> {
    pub node: N,
    pub interface: Rc<RefCell<SimulatedInterface>>,
    pub node_id: NodeId,
    /// History of state transitions: (Timestamp, LogMessage)
    pub state_log: Vec<(u64, String)>, 
}

impl<N: Node + Inspectable> NodeHarness<N> {
    pub fn new(node: N, interface: Rc<RefCell<SimulatedInterface>>, node_id: NodeId) -> Self {
        Self {
            node,
            interface,
            node_id,
            state_log: Vec::new(),
        }
    }

    /// runs a single cycle of the node logic
    pub fn run_cycle(&mut self, network: &mut VirtualNetwork) {
        // Capture state before cycle
        let prev_nmt = self.node.nmt_state(); // Now unambiguous (from Node trait)
        let prev_dll = self.node.dll_state_str();

        // 1. Check if we have a frame waiting in the network for us
        // We peel frames from the network inbox into the interface's internal rx queue
        while let Some(packet) = network.receive(self.node_id.0) {
            self.interface.borrow_mut().push_rx(packet.data);
        }
        
        // Now the interface has data. We "receive" it from the interface into a buffer.
        let mut rx_buffer = [0u8; 1518];
        let rx_len = match self.interface.borrow_mut().receive_frame(&mut rx_buffer) {
             Ok(len) => len,
             Err(_) => 0,
        };

        // 2. Run the node cycle
        #[cfg(feature = "sdo-udp")]
        let action = if rx_len > 0 {
             self.node.run_cycle(Some(&rx_buffer[..rx_len]), None, network.current_time())
        } else {
             self.node.run_cycle(None, None, network.current_time())
        };

        #[cfg(not(feature = "sdo-udp"))]
        let action = if rx_len > 0 {
             self.node.run_cycle(Some(&rx_buffer[..rx_len]), network.current_time())
        } else {
             self.node.run_cycle(None, network.current_time())
        };

        // Capture state after cycle
        let curr_nmt = self.node.nmt_state(); // Now unambiguous
        let curr_dll = self.node.dll_state_str();
        let current_time = network.current_time();

        // Log state changes
        if prev_nmt != curr_nmt || prev_dll != curr_dll {
            let log_msg = format!(
                "NMT: {:?} -> {:?} | DLL: {} -> {}",
                prev_nmt, curr_nmt, prev_dll, curr_dll
            );
            self.state_log.push((current_time, log_msg));
        }

        // 3. Handle output actions
        match action {
            NodeAction::SendFrame(frame) => {
                // Send via interface (which pushes to network)
                self.interface.borrow_mut().send_frame(&frame).unwrap();
                
                let tx_frames = self.interface.borrow_mut().take_tx_frames();
                for data in tx_frames {
                    // Simple broadcast logic for now.
                    network.transmit(Packet {
                        data,
                        src_node_id: self.node_id.0,
                        transmit_time_us: current_time,
                    }, None); // None = Broadcast
                }
            }
            _ => {}
        }
    }

    pub fn dump_state_log(&self, path: &str) -> std::io::Result<()> {
        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);

        writeln!(writer, "--- Node {} State Transition Log ---", self.node_id)?;
        for (time, msg) in &self.state_log {
            writeln!(writer, "[{:08} us] {}", time, msg)?;
        }
        Ok(())
    }
}