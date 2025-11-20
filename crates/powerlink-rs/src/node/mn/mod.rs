// crates/powerlink-rs/src/node/mn/mod.rs
pub(crate) mod config;
mod cycle;
mod events;
mod main;
mod payload;
mod scheduler;
mod state;
mod tick; // <-- ADDED
pub mod validation;

pub use main::ManagingNode;
pub use state::{CnInfo, CnState, MnContext};

use crate::{NodeId, types::IpAddress};

/// Helper to derive a CN's IP Address from its Node ID.
/// (Per EPSG DS 301, Section 5.1.2)
fn ip_from_node_id(node_id: NodeId) -> IpAddress {
    [192, 168, 100, node_id.0]
}