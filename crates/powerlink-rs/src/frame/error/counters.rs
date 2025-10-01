// In frame/error/counters.rs

use crate::types::NodeId;
use super::traits::{ErrorCounters, ErrorHandler};
use super::types::{DllError, NmtAction};
use alloc::collections::BTreeMap;

/// Implements the 8:1 threshold counter logic from the specification.
/// (EPSG DS 301, Section 4.7.4.1)
#[derive(Debug, Default)]
pub struct ThresholdCounter {
    count: u32,
    threshold: u32,
}

impl ThresholdCounter {
    /// Creates a new counter with a specific threshold.
    pub fn new(threshold: u32) -> Self {
        Self { count: 0, threshold }
    }

    /// Increments the counter by 8 when an error occurs[cite: 965].
    pub fn increment(&mut self) {
        self.count = self.count.saturating_add(8);
    }

    /// Decrements the counter by 1 for each error-free cycle[cite: 965].
    pub fn decrement(&mut self) {
        self.count = self.count.saturating_sub(1);
    }

    /// Checks if the threshold has been reached. If so, resets the counter
    /// and returns true[cite: 965].
    pub fn check_and_reset(&mut self) -> bool {
        if self.threshold > 0 && self.count >= self.threshold {
            self.count = 0;
            true
        } else {
            false
        }
    }
}

// --- Controlled Node (CN) Counters ---

/// Holds all DLL error counters for a Controlled Node.
/// These correspond to the object dictionary entries in Section 4.7.8.
#[derive(Debug)]
pub struct CnErrorCounters {
    pub loss_of_soc: ThresholdCounter,
    pub loss_of_soa: ThresholdCounter,
    pub loss_of_preq: ThresholdCounter,
    pub crc_errors: ThresholdCounter,
    pub collision: ThresholdCounter,
    pub soc_jitter: ThresholdCounter,
    // Cumulative counters do not reset.
    pub loss_of_link_cumulative: u32,
}

impl CnErrorCounters {
    /// Creates a new set of counters, with default thresholds from the specification.
    pub fn new() -> Self {
        CnErrorCounters {
            loss_of_soc: ThresholdCounter::new(15),
            loss_of_soa: ThresholdCounter::new(15),
            loss_of_preq: ThresholdCounter::new(15),
            crc_errors: ThresholdCounter::new(15),
            collision: ThresholdCounter::new(15),
            soc_jitter: ThresholdCounter::new(15),
            loss_of_link_cumulative: 0,
        }
    }
}

impl Default for CnErrorCounters {
    fn default() -> Self {
        Self::new()
    }
}

impl ErrorCounters for CnErrorCounters {
    fn on_cycle_complete(&mut self) {
        self.loss_of_soc.decrement();
        self.loss_of_soa.decrement();
        self.loss_of_preq.decrement();
        self.crc_errors.decrement();
        self.collision.decrement();
        self.soc_jitter.decrement();
    }
    
    fn handle_error<H: ErrorHandler>(&mut self, error: DllError, handler: &mut H) -> NmtAction {
        let threshold_reached = match error {
            DllError::E_DLL_LOSS_SOC_TH => {
                self.loss_of_soc.increment();
                self.loss_of_soc.check_and_reset()
            },
            DllError::E_DLL_LOSS_SOA_TH => {
                self.loss_of_soa.increment();
                self.loss_of_soa.check_and_reset()
            },
            DllError::E_DLL_LOSS_PREQ_TH => {
                self.loss_of_preq.increment();
                self.loss_of_preq.check_and_reset()
            },
            DllError::E_DLL_CRC_TH => {
                self.crc_errors.increment();
                self.crc_errors.check_and_reset()
            },
            DllError::E_DLL_COLLISION_TH => {
                self.collision.increment();
                self.collision.check_and_reset()
            },
            DllError::E_DLL_JITTER_TH => {
                self.soc_jitter.increment();
                self.soc_jitter.check_and_reset()
            },
            DllError::E_DLL_LOSS_OF_LINK => {
                self.loss_of_link_cumulative = self.loss_of_link_cumulative.saturating_add(1);
                handler.log_error(&error);
                false // Does not trigger an immediate NMT action.
            },
            // Errors handled by MN are ignored here.
            _ => false,
        };
        
        if threshold_reached {
            handler.log_error(&error);
            // Per Table 27, most threshold errors on a CN trigger a reset to PreOp1.
            return NmtAction::ResetCommunication;
        }
        NmtAction::None
    }
}

// --- Managing Node (MN) Counters ---

/// Holds all DLL error counters for a Managing Node.
#[derive(Debug, Default)]
pub struct MnErrorCounters {
    pub crc_errors: ThresholdCounter,
    pub collision: ThresholdCounter,
    pub cycle_time_exceeded: ThresholdCounter,
    // Per-CN counters
    pub loss_of_pres: BTreeMap<NodeId, ThresholdCounter>,
    pub late_pres: BTreeMap<NodeId, ThresholdCounter>,
    pub loss_of_status_response: BTreeMap<NodeId, ThresholdCounter>,
}

impl MnErrorCounters {
    pub fn new() -> Self { Self::default() }
    
    // Helper methods to get or insert a counter for a given node.
    fn loss_pres_counter_for(&mut self, node_id: NodeId) -> &mut ThresholdCounter {
        self.loss_of_pres.entry(node_id).or_insert_with(|| ThresholdCounter::new(15))
    }
    fn late_pres_counter_for(&mut self, node_id: NodeId) -> &mut ThresholdCounter {
        self.late_pres.entry(node_id).or_insert_with(|| ThresholdCounter::new(15))
    }
    fn loss_status_res_counter_for(&mut self, node_id: NodeId) -> &mut ThresholdCounter {
        self.loss_of_status_response.entry(node_id).or_insert_with(|| ThresholdCounter::new(15))
    }
}

impl ErrorCounters for MnErrorCounters {
    fn on_cycle_complete(&mut self) {
        self.crc_errors.decrement();
        self.collision.decrement();
        self.cycle_time_exceeded.decrement();
        self.loss_of_pres.values_mut().for_each(|c| c.decrement());
        self.late_pres.values_mut().for_each(|c| c.decrement());
        self.loss_of_status_response.values_mut().for_each(|c| c.decrement());
    }

    fn handle_error<H: ErrorHandler>(&mut self, error: DllError, handler: &mut H) -> NmtAction {
        let (threshold_reached, node_id) = match error {
            DllError::E_DLL_CRC_TH => {
                self.crc_errors.increment();
                (self.crc_errors.check_and_reset(), None)
            },
            DllError::E_DLL_COLLISION_TH => {
                self.collision.increment();
                (self.collision.check_and_reset(), None)
            },
            DllError::E_DLL_CYCLE_EXCEED_TH => {
                self.cycle_time_exceeded.increment();
                (self.cycle_time_exceeded.check_and_reset(), None)
            },
            DllError::E_DLL_LOSS_PRES_TH { node_id } => {
                let counter = self.loss_pres_counter_for(node_id);
                counter.increment();
                (counter.check_and_reset(), Some(node_id))
            },
            DllError::E_DLL_LATE_PRES_TH { node_id } => {
                let counter = self.late_pres_counter_for(node_id);
                counter.increment();
                (counter.check_and_reset(), Some(node_id))
            },
            DllError::E_DLL_LOSS_STATUSRES_TH { node_id } => {
                let counter = self.loss_status_res_counter_for(node_id);
                counter.increment();
                (counter.check_and_reset(), Some(node_id))
            },
            // Errors handled by CN are ignored here.
            _ => (false, None),
        };
        
        if threshold_reached {
            handler.log_error(&error);
            // Per Table 28, the MN's action depends on the error type.
            if let Some(id) = node_id {
                // For per-CN errors, reset the specific node.
                return NmtAction::ResetNode(id);
            } else {
                // For general MN errors, reset communication.
                return NmtAction::ResetCommunication;
            }
        }
        NmtAction::None
    }
}