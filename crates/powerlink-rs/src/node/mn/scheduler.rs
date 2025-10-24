// crates/powerlink-rs/src/node/mn/scheduler.rs
// Modified scheduler logic: Added multiplex check, improved state checks for NMT transitions.
use crate::node::Node; // Import Node trait for nmt_state()
use super::main::{CnState, ManagingNode};
use crate::nmt::{NmtEvent, NmtStateMachine, states::NmtState}; // Added NmtState import
use crate::types::NodeId;
use log::{debug, info, trace}; // Added trace import

/// Checks if MN can transition NMT state based on mandatory CN states.
pub(super) fn check_bootup_state(node: &mut ManagingNode) {
    let current_mn_state = node.nmt_state();

    if current_mn_state == NmtState::NmtPreOperational1 {
        // Check if all mandatory nodes are Identified or further, but not Missing or Stopped
        let all_mandatory_identified = node
            .mandatory_nodes
            .iter()
            .all(|node_id| {
                let state = node.node_states.get(node_id).copied().unwrap_or(CnState::Unknown);
                 // Updated condition: >= Identified AND <= Operational
                state >= CnState::Identified && state <= CnState::Operational
            });

        if all_mandatory_identified {
            info!("[MN] All mandatory nodes identified. Triggering NMT transition to PreOp2.");
            // NMT_MT3
            node.nmt_state_machine
                .process_event(NmtEvent::AllCnsIdentified, &mut node.od);
        }
    } else if current_mn_state == NmtState::NmtPreOperational2 {
        // Check if all mandatory nodes are PreOperational or further (ReadyToOp reported via PRes/Status)
        let all_mandatory_preop = node
            .mandatory_nodes
            .iter()
            .all(|node_id| {
                let state = node.node_states.get(node_id).copied().unwrap_or(CnState::Unknown);
                // CN reports PreOp2 or ReadyToOp, MN maps this to CnState::PreOperational
                // Also check <= Operational to ensure node hasn't gone missing/stopped
                state >= CnState::PreOperational && state <= CnState::Operational
            });

        if all_mandatory_preop {
             // Check MN startup flags if application trigger is needed
             // NMT_StartUp_U32.Bit8 = 0 -> Auto transition
             if node.nmt_state_machine.startup_flags & (1 << 8) == 0 {
                info!("[MN] All mandatory nodes PreOperational/ReadyToOp. Triggering NMT transition to ReadyToOp.");
                // NMT_MT4
                node.nmt_state_machine
                    .process_event(NmtEvent::ConfigurationCompleteCnsReady, &mut node.od);
             } else {
                 debug!("[MN] All mandatory nodes PreOperational/ReadyToOp, but waiting for application trigger to enter ReadyToOp.");
             }
        }
    } else if current_mn_state == NmtState::NmtReadyToOperate {
         // Check if all mandatory nodes are Operational
        let all_mandatory_operational = node
            .mandatory_nodes
            .iter()
            .all(|node_id| {
                let state = node.node_states.get(node_id).copied().unwrap_or(CnState::Unknown);
                state >= CnState::Operational
            });
         if all_mandatory_operational {
            // Check MN startup flags if application trigger is needed
            // NMT_StartUp_U32.Bit2 = 0 -> Auto transition
            if node.nmt_state_machine.startup_flags & (1 << 2) == 0 {
                info!("[MN] All mandatory nodes Operational. Triggering NMT transition to Operational.");
                 // NMT_MT5 - Use StartNode event as trigger for Operational state entry logic
                node.nmt_state_machine
                    .process_event(NmtEvent::StartNode, &mut node.od);
            } else {
                 debug!("[MN] All mandatory nodes Operational, but waiting for application trigger to enter Operational.");
            }
         }
    }
    // No automatic checks needed in Operational state based on CN states alone.
}

/// Finds the next configured CN that has not been identified yet for polling.
pub(super) fn find_next_node_to_identify(node: &mut ManagingNode) -> Option<NodeId> {
    // Start iterating from the node *after* the last one polled
    let start_node_id_val = node.last_ident_poll_node_id.0.wrapping_add(1); // Access pub(super) field

    let mut wrapped_around = false;
    let mut current_node_id_val = start_node_id_val;

    loop {
        // Handle wrap-around and node ID range (1-239 for CNs)
        if current_node_id_val == 0 || current_node_id_val > 239 {
            current_node_id_val = 1;
        }
        if current_node_id_val == start_node_id_val {
            if wrapped_around {
                debug!("[MN] Full circle check for unidentified nodes completed.");
                break; // Full circle, no nodes found
            }
            wrapped_around = true;
        }

        // Ensure NodeId::try_from is used or logic handles invalid IDs
        let node_id = NodeId(current_node_id_val); // Directly create NodeId

        // Check if this node ID exists in our configured node state map
        // AND if its current state is Unknown
        if node.node_states.get(&node_id) == Some(&CnState::Unknown) { // Access pub(super) field
            // Found a node to poll
            debug!("[MN] Found unidentified Node {} to poll.", node_id.0);
            node.last_ident_poll_node_id = node_id; // Access pub(super) field
            return Some(node_id);
        }

        current_node_id_val = current_node_id_val.wrapping_add(1);
    }

    debug!("[MN] No more unidentified nodes found.");
    None // No unidentified nodes left
}

/// Gets the Node ID of the next isochronous node to poll for the given multiplex cycle.
/// Returns None if all nodes for the current cycle have been polled.
/// This function modifies the internal `next_isoch_node_idx`.
pub(super) fn get_next_isochronous_node_to_poll(node: &mut ManagingNode, current_multiplex_cycle: u8) -> Option<NodeId> {
    // Iterate through the pre-defined list starting from the current index
    while node.next_isoch_node_idx < node.isochronous_nodes.len() { // Access pub(super) fields
        let node_id = node.isochronous_nodes[node.next_isoch_node_idx]; // Access pub(super) fields
        node.next_isoch_node_idx += 1; // Move to the next index for the *next* call // Access pub(super) field

        // Check if this node should be polled in the current multiplex cycle
        let assigned_cycle = node.multiplex_assign.get(&node_id).copied().unwrap_or(0);
        // Corrected multiplex check: cycle counter is 0-based, assigned is 1-based
        let should_poll_this_cycle = assigned_cycle == 0 // Continuous node
            || (node.multiplex_cycle_len > 0 && assigned_cycle == (current_multiplex_cycle + 1)); // Multiplexed node for this cycle (assigned cycle is 1-based)

        if should_poll_this_cycle {
            // Check if the node is in a state where it should be polled isochronously
            let state = node.node_states.get(&node_id).copied().unwrap_or(CnState::Unknown); // Access pub(super) field
            // Poll nodes from Identified onwards, excluding Stopped/Missing
            // PReq allowed in PreOp2, ReadyToOp, Operational
            // Corrected state check: >= PreOperational
            if state >= CnState::PreOperational {
                // Found a valid node to poll in this cycle
                trace!("[MN] Polling Node {} (State: {:?}, MuxCycle: {}) in mux cycle {}", node_id.0, state, assigned_cycle, current_multiplex_cycle);
                return Some(node_id);
            } else {
                 debug!("[MN] Skipping Node {} (State: {:?}) in isochronous polling for mux cycle {}.", node_id.0, state, current_multiplex_cycle);
            }
        } else {
             trace!("[MN] Skipping Node {} (assigned mux cycle {}) in current mux cycle {}.", node_id.0, assigned_cycle, current_multiplex_cycle);
        }
        // If the node is not in a pollable state or not for this cycle, the loop continues
    }
    None // No more nodes left to poll in this cycle
}

/// Helper to check if there are more isochronous nodes to poll in the current cycle.
/// Does not modify `next_isoch_node_idx`.
pub(super) fn has_more_isochronous_nodes(node: &ManagingNode, current_multiplex_cycle: u8) -> bool {
    // Check remaining nodes in the list from the current index
    for idx in node.next_isoch_node_idx..node.isochronous_nodes.len() {
        let node_id = node.isochronous_nodes[idx];
        let assigned_cycle = node.multiplex_assign.get(&node_id).copied().unwrap_or(0);
        // Corrected multiplex check
        let should_poll_this_cycle = assigned_cycle == 0
            || (node.multiplex_cycle_len > 0 && assigned_cycle == (current_multiplex_cycle + 1));

        if should_poll_this_cycle {
            let state = node.node_states.get(&node_id).copied().unwrap_or(CnState::Unknown);
            // Corrected state check: >= PreOperational
            if state >= CnState::PreOperational {
                return true; // Found at least one more node to poll
            }
        }
    }
    false // No more pollable nodes found for this cycle
}

