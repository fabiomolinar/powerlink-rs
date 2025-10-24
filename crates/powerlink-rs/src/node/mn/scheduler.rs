// crates/powerlink-rs/src/node/mn/scheduler.rs
use crate::node::Node; // Import Node trait for nmt_state()
use super::main::{CnState, ManagingNode};
use crate::nmt::{NmtEvent, NmtStateMachine};
use crate::types::NodeId;
use log::{debug, info};

/// Checks if all mandatory nodes are identified to trigger transition to PreOp2.
pub(super) fn check_bootup_state(node: &mut ManagingNode) {
    // Use trait method, now in scope
    if node.nmt_state() != crate::nmt::states::NmtState::NmtPreOperational1 {
        return; // Only check this in PreOp1
    }

    let all_mandatory_identified = node
        .mandatory_nodes // Access pub(super) field
        .iter()
        .all(|node_id| node.node_states.get(node_id).copied().unwrap_or(CnState::Unknown) >= CnState::Identified); // Check if Identified or later state

    if all_mandatory_identified {
        info!("[MN] All mandatory nodes identified. Transitioning to PreOp2.");
        // Use the NmtStateMachine trait method
        node.nmt_state_machine // Access pub(super) field
            .process_event(NmtEvent::AllCnsIdentified, &mut node.od);
    }
}

/// Finds the next configured CN that has not been identified yet for polling.
pub(super) fn find_next_node_to_identify(node: &mut ManagingNode) -> Option<NodeId> {
    // Start iterating from the node *after* the last one polled
    let start_node_id = node.last_ident_poll_node_id.0.wrapping_add(1); // Access pub(super) field

    let mut wrapped_around = false;
    let mut current_node_id = start_node_id;

    loop {
        // Handle wrap-around and node ID range (1-239 for CNs)
        if current_node_id == 0 || current_node_id > 239 {
            current_node_id = 1;
        }
        if current_node_id == start_node_id {
            if wrapped_around {
                debug!("[MN] Full circle check for unidentified nodes completed.");
                break; // Full circle, no nodes found
            }
            wrapped_around = true;
        }
        
        // Ensure NodeId::try_from is used or logic handles invalid IDs
        let node_id = NodeId(current_node_id);
        
        // Check if this node ID exists in our configured node state map
        // AND if its current state is Unknown
        if node.node_states.get(&node_id) == Some(&CnState::Unknown) { // Access pub(super) field
            // Found a node to poll
            debug!("[MN] Found unidentified Node {} to poll.", node_id.0);
            node.last_ident_poll_node_id = node_id; // Access pub(super) field
            return Some(node_id);
        }

        current_node_id = current_node_id.wrapping_add(1);
    }

    debug!("[MN] No more unidentified nodes found.");
    None // No unidentified nodes left
}

/// Gets the Node ID of the next isochronous node to poll.
/// Returns None if all nodes for the current cycle have been polled.
/// This function is intended to be called iteratively within the MN's tick/scheduler.
/// It modifies the internal `next_isoch_node_idx`.
pub(super) fn get_next_isochronous_node_to_poll(node: &mut ManagingNode) -> Option<NodeId> {
    // Iterate through the pre-defined list starting from the current index
    while node.next_isoch_node_idx < node.isochronous_nodes.len() { // Access pub(super) fields
        let node_id = node.isochronous_nodes[node.next_isoch_node_idx]; // Access pub(super) fields
        node.next_isoch_node_idx += 1; // Move to the next index for the *next* call // Access pub(super) field

        // Check if the node is in a state where it should be polled isochronously
        let state = node.node_states.get(&node_id).copied().unwrap_or(CnState::Unknown); // Access pub(super) field
        // Poll nodes from Identified onwards, excluding Stopped/Missing
        if state >= CnState::Identified && state != CnState::Stopped && state != CnState::Missing {
            // Found a valid node to poll in this cycle
            return Some(node_id);
        } else {
             debug!("[MN] Skipping Node {} (State: {:?}) in isochronous polling.", node_id.0, state);
        }
        // If the node is not in a pollable state, the loop continues to the next index
    }
    None // No more nodes left to poll in this cycle
}

