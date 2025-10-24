// crates/powerlink-rs/src/node/mn/scheduler.rs

use super::main::{CnState, ManagingNode};
use crate::Node;
use crate::nmt::{NmtEvent, NmtStateMachine}; // Added NmtStateMachine import
use crate::types::NodeId;
use log::info;

/// Checks if all mandatory nodes are identified to trigger transition to PreOp2.
pub(super) fn check_bootup_state(node: &mut ManagingNode) {
    if node.nmt_state() != crate::nmt::states::NmtState::NmtPreOperational1 {
        return; // Only check this in PreOp1
    }

    let all_mandatory_identified = node
        .mandatory_nodes
        .iter()
        .all(|node_id| node.node_states.get(node_id) == Some(&CnState::Identified));

    if all_mandatory_identified {
        info!("[MN] All mandatory nodes identified. Transitioning to PreOp2.");
        // Use the NmtStateMachine trait method
        node.nmt_state_machine
            .process_event(NmtEvent::AllCnsIdentified, &mut node.od);
    }
}

/// Finds the next configured CN that has not been identified yet for polling.
pub(super) fn find_next_node_to_identify(node: &mut ManagingNode) -> Option<NodeId> {
    // Start iterating from the node *after* the last one polled
    let start_node_id = node.last_ident_poll_node_id.0.wrapping_add(1);

    let mut wrapped_around = false;
    let mut current_node_id = start_node_id;

    loop {
        // Handle wrap-around and node ID range (1-239 for CNs)
        if current_node_id == 0 || current_node_id > 239 {
            current_node_id = 1;
        }
        if current_node_id == start_node_id {
            if wrapped_around {
                break; // Full circle, no nodes found
            }
            wrapped_around = true;
        }

        let node_id = NodeId(current_node_id);
        // Check if this node ID exists in our configured node state map
        // AND if its current state is Unknown
        if node.node_states.get(&node_id) == Some(&CnState::Unknown) {
            // Found a node to poll
            node.last_ident_poll_node_id = node_id;
            return Some(node_id);
        }

        current_node_id = current_node_id.wrapping_add(1);
    }

    None // No unidentified nodes left
}
