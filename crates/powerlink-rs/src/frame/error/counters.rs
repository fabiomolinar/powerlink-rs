// In frame/error/counters.rs

use crate::types::NodeId;
use super::traits::{ErrorCounters, ErrorHandler};
use super::types::{DllError, NmtAction};
use alloc::collections::BTreeMap;

/// Implements the 8:1 threshold counter logic from the specification.
/// (EPSG DS 301, Section 4.7.4.1)
#[derive(Debug, Default)]
pub struct ThresholdCounter {
    cumulative_cnt: u32,
    threshold_cnt: u32,
    threshold: u32,
}

impl ThresholdCounter {
    /// Creates a new counter with a specific threshold.
    pub fn new(threshold: u32) -> Self {
        Self { 
            cumulative_cnt: 0,
            threshold_cnt: 0,
            threshold,
         }
    }

    /// Increments the counter by 8 when an error occurs[cite: 965].
    pub fn increment(&mut self) {
        self.threshold_cnt = self.threshold_cnt.saturating_add(8);
    }

    /// Decrements the counter by 1 for each error-free cycle[cite: 965].
    pub fn decrement(&mut self) {
        self.threshold_cnt = self.threshold_cnt.saturating_sub(1);
    }

    /// Checks if the threshold has been reached. If so, resets the counter
    /// and returns true[cite: 965].
    pub fn check_and_reset(&mut self) -> bool {
        if self.threshold > 0 && self.threshold_cnt >= self.threshold {
            self.threshold_cnt = 0;
            self.cumulative_cnt = self.cumulative_cnt.saturating_add(1);
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
    pub collision: ThresholdCounter,
    pub loss_of_soc: ThresholdCounter,
    pub loss_of_soa: ThresholdCounter,
    pub loss_of_preq: ThresholdCounter,
    pub soc_jitter: ThresholdCounter,
    pub crc_errors: ThresholdCounter,
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
            DllError::LossOfSocThreshold => {
                self.loss_of_soc.increment();
                self.loss_of_soc.check_and_reset()
            },
            DllError::LossOfSoaThreshold => {
                self.loss_of_soa.increment();
                self.loss_of_soa.check_and_reset()
            },
            DllError::LossOfPreqThreshold => {
                self.loss_of_preq.increment();
                self.loss_of_preq.check_and_reset()
            },
            DllError::CrcThreshold => {
                self.crc_errors.increment();
                self.crc_errors.check_and_reset()
            },
            DllError::CollisionThreshold => {
                self.collision.increment();
                self.collision.check_and_reset()
            },
            DllError::JitterThreshold => {
                self.soc_jitter.increment();
                self.soc_jitter.check_and_reset()
            },
            DllError::LossOfLink => {
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
    // Cumulative counters do not reset.
    pub loss_of_link_cumulative: u32,    
    // Per-CN counters    
    pub cn_late_pres: BTreeMap<NodeId, ThresholdCounter>,
    pub cn_loss_of_pres: BTreeMap<NodeId, ThresholdCounter>,    
    pub cn_loss_of_status_response: BTreeMap<NodeId, ThresholdCounter>,
    // Cumulative counters do not reset.
}

impl MnErrorCounters {
    pub fn new() -> Self { Self::default() }
    
    // Helper methods to get or insert a counter for a given node.
    fn loss_pres_counter_for(&mut self, node_id: NodeId) -> &mut ThresholdCounter {
        self.cn_loss_of_pres.entry(node_id).or_insert_with(|| ThresholdCounter::new(15))
    }
    fn late_pres_counter_for(&mut self, node_id: NodeId) -> &mut ThresholdCounter {
        self.cn_late_pres.entry(node_id).or_insert_with(|| ThresholdCounter::new(15))
    }
    fn loss_status_res_counter_for(&mut self, node_id: NodeId) -> &mut ThresholdCounter {
        self.cn_loss_of_status_response.entry(node_id).or_insert_with(|| ThresholdCounter::new(15))
    }
}

impl ErrorCounters for MnErrorCounters {
    fn on_cycle_complete(&mut self) {
        self.crc_errors.decrement();
        self.collision.decrement();
        self.cycle_time_exceeded.decrement();
        self.cn_loss_of_pres.values_mut().for_each(|c| c.decrement());
        self.cn_late_pres.values_mut().for_each(|c| c.decrement());
        self.cn_loss_of_status_response.values_mut().for_each(|c| c.decrement());
    }

    fn handle_error<H: ErrorHandler>(&mut self, error: DllError, handler: &mut H) -> NmtAction {
        let (threshold_reached, node_id) = match error {
            DllError::CrcThreshold => {
                self.crc_errors.increment();
                (self.crc_errors.check_and_reset(), None)
            },
            DllError::CollisionThreshold => {
                self.collision.increment();
                (self.collision.check_and_reset(), None)
            },
            DllError::CycleExceededThreshold => {
                self.cycle_time_exceeded.increment();
                (self.cycle_time_exceeded.check_and_reset(), None)
            },
            DllError::LossOfPresThreshold { node_id } => {
                let counter = self.loss_pres_counter_for(node_id);
                counter.increment();
                (counter.check_and_reset(), Some(node_id))
            },
            DllError::LatePresThreshold { node_id } => {
                let counter = self.late_pres_counter_for(node_id);
                counter.increment();
                (counter.check_and_reset(), Some(node_id))
            },
            DllError::LossOfStatusResThreshold { node_id } => {
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

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;
    
    // A mock error handler for testing purposes.
    struct TestErrorHandler {
        logged_errors: Vec<DllError>,
    }
    impl ErrorHandler for TestErrorHandler {
        fn log_error(&mut self, error: &DllError) {
            self.logged_errors.push(*error);
        }
    }

    #[test]
    fn test_threshold_counter_logic() {
        let mut counter = ThresholdCounter::new(10);

        // Increment once, count should be 8.
        counter.increment();
        assert_eq!(counter.threshold_cnt, 8);
        assert!(!counter.check_and_reset());

        // Decrement, count should be 7.
        counter.decrement();
        assert_eq!(counter.threshold_cnt, 7);

        // Increment again, count becomes 15, which is >= 10.
        counter.increment();
        assert_eq!(counter.threshold_cnt, 15);

        // Check should now return true and reset the count to 0.
        assert!(counter.check_and_reset());
        assert_eq!(counter.threshold_cnt, 0);
    }

    #[test]
    fn test_cn_error_counters_handling() {
        let mut counters = CnErrorCounters::new();
        let mut handler = TestErrorHandler { logged_errors: Vec::new() };

        // First error, no action yet.
        let action1 = counters.handle_error(DllError::LossOfSocThreshold, &mut handler);
        assert_eq!(action1, NmtAction::None);
        assert_eq!(handler.logged_errors.len(), 0);

        // Second error, threshold is now 16 (>= 15), so an action is triggered.
        let action2 = counters.handle_error(DllError::LossOfSocThreshold, &mut handler);
        assert_eq!(action2, NmtAction::ResetCommunication);
        assert_eq!(handler.logged_errors.len(), 1);
        assert_eq!(handler.logged_errors[0], DllError::LossOfSocThreshold);
    }

    #[test]
    fn test_mn_error_counters_handling() {
        let mut counters = MnErrorCounters::new();
        let mut handler = TestErrorHandler { logged_errors: Vec::new() };
        let node_id = NodeId(5);
        let error = DllError::LossOfPresThreshold { node_id };

        // Trigger the error twice to exceed the threshold.
        let action1 = counters.handle_error(error, &mut handler);
        let action2 = counters.handle_error(error, &mut handler);

        assert_eq!(action1, NmtAction::None);
        assert_eq!(action2, NmtAction::ResetNode(node_id));
        assert_eq!(handler.logged_errors.len(), 1);
        assert_eq!(handler.logged_errors[0], error);
    }

    #[test]
    fn test_counters_on_cycle_complete() {
        let mut cn_counters = CnErrorCounters::new();
        cn_counters.loss_of_soc.increment(); // count = 8
        cn_counters.on_cycle_complete();
        assert_eq!(cn_counters.loss_of_soc.threshold_cnt, 7);

        let mut mn_counters = MnErrorCounters::new();
        let node_id = NodeId(10);
        mn_counters.handle_error(DllError::LossOfPresThreshold { node_id }, &mut TestErrorHandler {logged_errors: Vec::new()});
        assert_eq!(mn_counters.cn_loss_of_pres.get(&node_id).unwrap().threshold_cnt, 8);
        mn_counters.on_cycle_complete();
        assert_eq!(mn_counters.cn_loss_of_pres.get(&node_id).unwrap().threshold_cnt, 7);
    }
}