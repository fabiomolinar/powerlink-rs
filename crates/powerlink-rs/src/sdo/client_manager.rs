// crates/powerlink-rs/src/sdo/client_manager.rs
//! Manages multiple, concurrent, stateful SDO client connections.
//!
//! This is primarily used by the Managing Node (MN) to perform complex
//! SDO transfers (like segmented downloads for CFM/PDL) to multiple CNs
//! simultaneously.

use crate::PowerlinkError;
use crate::od::ObjectDictionary;
use crate::sdo::client_connection::SdoClientConnection;
use crate::sdo::command::SdoCommand;
use crate::sdo::sequence::SequenceLayerHeader;
use crate::types::NodeId;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;

#[derive(Debug, Default)]
pub struct SdoClientManager {
    connections: BTreeMap<NodeId, SdoClientConnection>,
    next_transaction_id: u8,
}

impl SdoClientManager {
    pub fn new() -> Self {
        Self::default()
    }

    fn get_next_tid(&mut self) -> u8 {
        self.next_transaction_id = self.next_transaction_id.wrapping_add(1);
        if self.next_transaction_id == 0 {
            self.next_transaction_id = 1;
        }
        self.next_transaction_id
    }

    pub fn next_action_time(&self, _od: &ObjectDictionary) -> Option<u64> {
        self.connections
            .values()
            .filter_map(|c| c.deadline_us)
            .min()
    }

    /// Starts a configuration download job (Concise DCF) for the target node.
    pub fn start_configuration_download(
        &mut self,
        target: NodeId,
        dcf_data: Vec<u8>,
        current_time_us: u64,
        od: &ObjectDictionary,
    ) -> Result<(), PowerlinkError> {
        let tid = self.get_next_tid();
        let conn = self
            .connections
            .entry(target)
            .or_insert_with(|| SdoClientConnection::new(target));
        conn.start_concise_dcf_job(dcf_data, tid, current_time_us, od)
    }

    pub fn read_object_by_index(
        &mut self,
        target: NodeId,
        index: u16,
        sub_index: u8,
        time: u64,
        od: &ObjectDictionary,
    ) -> Result<(), PowerlinkError> {
        let tid = self.get_next_tid();
        let conn = self
            .connections
            .entry(target)
            .or_insert_with(|| SdoClientConnection::new(target));
        conn.start_read_job(index, sub_index, tid, time, od)
    }

    pub fn write_object_by_index(
        &mut self,
        target: NodeId,
        index: u16,
        sub_index: u8,
        data: Vec<u8>,
        time: u64,
        od: &ObjectDictionary,
    ) -> Result<(), PowerlinkError> {
        let tid = self.get_next_tid();
        let conn = self
            .connections
            .entry(target)
            .or_insert_with(|| SdoClientConnection::new(target));
        conn.start_write_job(index, sub_index, data, tid, time, od)
    }

    pub fn handle_response(&mut self, source: NodeId, seq: SequenceLayerHeader, cmd: SdoCommand) {
        if let Some(conn) = self.connections.get_mut(&source) {
            conn.handle_response(&seq, &cmd);
            if conn.is_closed() {
                self.connections.remove(&source);
            }
        }
    }

    pub fn tick(
        &mut self,
        time: u64,
        od: &ObjectDictionary,
    ) -> Option<(NodeId, SequenceLayerHeader, SdoCommand)> {
        let mut res = None;
        let mut prune = Vec::new();
        for (id, conn) in self.connections.iter_mut() {
            if res.is_none() {
                if let Some(out) = conn.tick(time, od) {
                    res = Some((*id, out.0, out.1));
                }
            }
            if conn.is_closed() {
                prune.push(*id);
            }
        }
        for id in prune {
            self.connections.remove(&id);
        }
        res
    }

    pub fn get_pending_request(
        &mut self,
        time: u64,
        od: &ObjectDictionary,
    ) -> Option<(NodeId, SequenceLayerHeader, SdoCommand)> {
        let mut res = None;
        let mut prune = Vec::new();
        for (id, conn) in self.connections.iter_mut() {
            if res.is_none() {
                if let Some(out) = conn.get_pending_request(time, od) {
                    res = Some((*id, out.0, out.1));
                }
            }
            if conn.is_closed() {
                prune.push(*id);
            }
        }
        for id in prune {
            self.connections.remove(&id);
        }
        res
    }
}
