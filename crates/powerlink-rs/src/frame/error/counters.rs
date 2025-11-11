use super::traits::{ErrorCounters, ErrorHandler};
use super::types::{DllError, NmtAction};
use crate::types::NodeId;
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

    /// Increments the counter by 8 when an error occurs.
    pub fn increment(&mut self) {
        self.threshold_cnt = self.threshold_cnt.saturating_add(8);
    }

    /// Decrements the counter by 1 for each error-free cycle.
    pub fn decrement(&mut self) {
        self.threshold_cnt = self.threshold_cnt.saturating_sub(1);
    }

    /// Checks if the threshold has been reached. If so, resets the counter
    /// and returns true.
    pub fn check_and_reset(&mut self) -> bool {
        if self.threshold > 0 && self.threshold_cnt >= self.threshold {
            self.threshold_cnt = 0;
            self.cumulative_cnt = self.cumulative_cnt.saturating_add(1);
            true
        } else {
            false
        }
    }

    /// Returns true if the threshold counter is greater than zero.
    pub fn is_active(&self) -> bool {
        self.threshold_cnt > 0
    }

    pub fn cumulative_count(&self) -> u32 {
        self.cumulative_cnt
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
    pub heartbeat_timeout: ThresholdCounter,
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
            heartbeat_timeout: ThresholdCounter::new(15), // Added heartbeat counter
            loss_of_link_cumulative: 0,
        }
    }
    /// Checks if any of the threshold counters are currently active ( > 0).
    fn is_any_active(&self) -> bool {
        self.loss_of_soc.is_active()
            || self.loss_of_soa.is_active()
            || self.loss_of_preq.is_active()
            || self.crc_errors.is_active()
            || self.collision.is_active()
            || self.soc_jitter.is_active()
            || self.heartbeat_timeout.is_active() // Added heartbeat check
    }
}

impl Default for CnErrorCounters {
    fn default() -> Self {
        Self::new()
    }
}

impl ErrorCounters for CnErrorCounters {
    fn on_cycle_complete(&mut self) -> bool {
        let was_active = self.is_any_active();

        self.loss_of_soc.decrement();
        self.loss_of_soa.decrement();
        self.loss_of_preq.decrement();
        self.crc_errors.decrement();
        self.collision.decrement();
        self.soc_jitter.decrement();
        self.heartbeat_timeout.decrement(); // Added heartbeat decrement

        let is_still_active = self.is_any_active();

        // Return true only if the state changed from active to inactive.
        was_active && !is_still_active
    }

    fn handle_error<H: ErrorHandler>(
        &mut self,
        error: DllError,
        handler: &mut H,
    ) -> (NmtAction, bool) {
        let mut nmt_action = NmtAction::None;
        let mut status_changed = false;

        let threshold_reached = match error {
            DllError::LossOfSoc => {
                self.loss_of_soc.increment();
                self.loss_of_soc.check_and_reset()
            }
            DllError::LossOfSoa => {
                self.loss_of_soa.increment();
                self.loss_of_soa.check_and_reset()
            }
            DllError::LossOfPreq => {
                self.loss_of_preq.increment();
                self.loss_of_preq.check_and_reset()
            }
            DllError::Crc => {
                self.crc_errors.increment();
                self.crc_errors.check_and_reset()
            }
            DllError::Collision => {
                self.collision.increment();
                self.collision.check_and_reset()
            }
            DllError::SoCJitter => {
                self.soc_jitter.increment();
                self.soc_jitter.check_and_reset()
            }
            // Added handler for HeartbeatTimeout
            DllError::HeartbeatTimeout { .. } => {
                self.heartbeat_timeout.increment();
                self.heartbeat_timeout.check_and_reset()
            }
            DllError::LossOfLink => {
                self.loss_of_link_cumulative = self.loss_of_link_cumulative.saturating_add(1);
                handler.log_error(&error);
                status_changed = true; // Loss of link is a signallable event.
                false // Does not trigger an immediate NMT action.
            }
            // PDO errors are logged but do not trigger threshold-based NMT actions
            DllError::PdoMapVersion { .. } | DllError::PdoPayloadShort { .. } => {
                handler.log_error(&error);
                status_changed = true; // PDO errors are signallable.
                false
            }
            // Errors handled by MN are ignored here.
            _ => false,
        };

        if threshold_reached {
            handler.log_error(&error);
            status_changed = true; // Any threshold violation is a signallable event.
            // Per Table 27, most threshold errors on a CN trigger a reset to PreOp1.
            nmt_action = NmtAction::ResetCommunication;
        }
        (nmt_action, status_changed)
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
    pub fn new() -> Self {
        Self::default()
    }

    // Helper methods to get or insert a counter for a given node.
    fn loss_pres_counter_for(&mut self, node_id: NodeId) -> &mut ThresholdCounter {
        self.cn_loss_of_pres
            .entry(node_id)
            .or_insert_with(|| ThresholdCounter::new(15))
    }
    fn late_pres_counter_for(&mut self, node_id: NodeId) -> &mut ThresholdCounter {
        self.cn_late_pres
            .entry(node_id)
            .or_insert_with(|| ThresholdCounter::new(15))
    }
    fn loss_status_res_counter_for(&mut self, node_id: NodeId) -> &mut ThresholdCounter {
        self.cn_loss_of_status_response
            .entry(node_id)
            .or_insert_with(|| ThresholdCounter::new(15))
    }
}

impl ErrorCounters for MnErrorCounters {
    fn on_cycle_complete(&mut self) -> bool {
        self.crc_errors.decrement();
        self.collision.decrement();
        self.cycle_time_exceeded.decrement();
        self.cn_loss_of_pres
            .values_mut()
            .for_each(|c| c.decrement());
        self.cn_late_pres.values_mut().for_each(|c| c.decrement());
        self.cn_loss_of_status_response
            .values_mut()
            .for_each(|c| c.decrement());
        // The MN does not signal its own error state in the same way a CN does,
        // so returning false is appropriate here.
        false
    }

    fn handle_error<H: ErrorHandler>(
        &mut self,
        error: DllError,
        handler: &mut H,
    ) -> (NmtAction, bool) {
        let mut nmt_action = NmtAction::None;
        let mut status_changed = false;

        let (threshold_reached, node_id) = match error {
            DllError::Crc => {
                self.crc_errors.increment();
                (self.crc_errors.check_and_reset(), None)
            }
            DllError::Collision => {
                self.collision.increment();
                (self.collision.check_and_reset(), None)
            }
            DllError::CycleTimeExceeded => {
                self.cycle_time_exceeded.increment();
                (self.cycle_time_exceeded.check_and_reset(), None)
            }
            DllError::LossOfPres { node_id } => {
                let counter = self.loss_pres_counter_for(node_id);
                counter.increment();
                (counter.check_and_reset(), Some(node_id))
            }
            DllError::LatePres { node_id } => {
                let counter = self.late_pres_counter_for(node_id);
                counter.increment();
                (counter.check_and_reset(), Some(node_id))
            }
            DllError::LossOfStatusRes { node_id } => {
                let counter = self.loss_status_res_counter_for(node_id);
                counter.increment();
                (counter.check_and_reset(), Some(node_id))
            }
            // PDO errors are logged but do not trigger threshold-based NMT actions
            DllError::PdoMapVersion { .. } | DllError::PdoPayloadShort { .. } => {
                handler.log_error(&error);
                status_changed = true;
                (false, None)
            }
            // Errors handled by CN are ignored here.
            _ => (false, None),
        };

        if threshold_reached {
            handler.log_error(&error);
            status_changed = true;
            // Per Table 28, the MN's action depends on the error type.
            if let Some(id) = node_id {
                // For per-CN errors, reset the specific node.
                nmt_action = NmtAction::ResetNode(id);
            } else {
                // For general MN errors, reset communication.
                nmt_action = NmtAction::ResetCommunication;
            }
        }
        (nmt_action, status_changed)
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
        let mut handler = TestErrorHandler {
            logged_errors: Vec::new(),
        };

        // First error, no action yet.
        let (action1, changed1) = counters.handle_error(DllError::LossOfSoc, &mut handler);
        assert_eq!(action1, NmtAction::None);
        assert!(!changed1);
        assert_eq!(handler.logged_errors.len(), 0);

        // Second error, threshold is now 16 (>= 15), so an action is triggered.
        let (action2, changed2) = counters.handle_error(DllError::LossOfSoc, &mut handler);
        assert_eq!(action2, NmtAction::ResetCommunication);
        assert!(changed2);
        assert_eq!(handler.logged_errors.len(), 1);
        assert_eq!(handler.logged_errors[0], DllError::LossOfSoc);
    }

    #[test]
    fn test_mn_error_counters_handling() {
        let mut counters = MnErrorCounters::new();
        let mut handler = TestErrorHandler {
            logged_errors: Vec::new(),
        };
        let node_id = NodeId(5);
        let error = DllError::LossOfPres { node_id };

        // Trigger the error twice to exceed the threshold.
        let (action1, changed1) = counters.handle_error(error, &mut handler);
        let (action2, changed2) = counters.handle_error(error, &mut handler);

        assert_eq!(action1, NmtAction::None);
        assert!(!changed1);
        assert_eq!(action2, NmtAction::ResetNode(node_id));
        assert!(changed2);
        assert_eq!(handler.logged_errors.len(), 1);
        assert_eq!(handler.logged_errors[0], error);
    }

    #[test]
    fn test_counters_on_cycle_complete() {
        let mut cn_counters = CnErrorCounters::new();
        cn_counters.loss_of_soc.increment(); // count = 8
        assert!(cn_counters.is_any_active());

        // Decrement 7 times, should still be active and return false
        for _ in 0..7 {
            assert!(!cn_counters.on_cycle_complete());
        }
        assert_eq!(cn_counters.loss_of_soc.threshold_cnt, 1);
        assert!(cn_counters.is_any_active());

        // 8th decrement clears the error, should return true
        assert!(cn_counters.on_cycle_complete());
        assert_eq!(cn_counters.loss_of_soc.threshold_cnt, 0);
        assert!(!cn_counters.is_any_active());

        // A further decrement when inactive should return false
        assert!(!cn_counters.on_cycle_complete());

        let mut mn_counters = MnErrorCounters::new();
        let node_id = NodeId(10);
        mn_counters.handle_error(
            DllError::LossOfPres { node_id },
            &mut TestErrorHandler {
                logged_errors: Vec::new(),
            },
        );
        assert_eq!(
            mn_counters
                .cn_loss_of_pres
                .get(&node_id)
                .unwrap()
                .threshold_cnt,
            8
        );
        // MN's on_cycle_complete always returns false
        assert!(!mn_counters.on_cycle_complete());
        assert_eq!(
            mn_counters
                .cn_loss_of_pres
                .get(&node_id)
                .unwrap()
                .threshold_cnt,
            7
        );
    }
}
