#![allow(non_camel_case_types)]
use crate::types::NodeId;
use alloc::collections::BTreeMap;

// --- 1. The Pluggable ErrorHandler Trait ---

/// A trait that defines how DLL errors are reported or logged.
pub trait ErrorHandler {
    /// Called by the DllErrorManager when a threshold is exceeded or a critical
    /// event occurs that requires logging or external action.
    fn log_error(&mut self, error: &DllError);
}

/// A `no_std` compatible error handler that does nothing.
/// This is the default for embedded targets where logging might not be available or desired.
pub struct NoOpErrorHandler;

impl ErrorHandler for NoOpErrorHandler {
    fn log_error(&mut self, _error: &DllError) {
        // This implementation intentionally does nothing.
    }
}

/// An example `std`-based error handler that prints errors to the console.
#[cfg(feature = "std")]
pub struct StdoutErrorHandler;

#[cfg(feature = "std")]
impl ErrorHandler for StdoutErrorHandler {
    fn log_error(&mut self, error: &DllError) {
        println!("[POWERLINK DLL ERROR]: {:?}", error);
    }
}


// --- 2. DLL Error Definitions ---

/// An action for the NMT layer to take in response to a critical DLL error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NmtAction {
    None,
    ResetCommunication,
    ResetNode(NodeId),
}

/// Represents all possible DLL error symptoms and sources.
/// (EPSG DS 301, Section 4.7)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DllError {
    E_DLL_LOSS_OF_LINK,
    E_DLL_BAD_PHYS_MODE,
    E_DLL_MAC_BUFFER,
    E_DLL_CRC_TH,
    E_DLL_COLLISION_TH,
    E_DLL_INVALID_FORMAT,
    E_DLL_LOSS_SOC_TH,
    E_DLL_LOSS_SOA_TH,
    E_DLL_LOSS_PREQ_TH,
    E_DLL_LOSS_PRES_TH { node_id: NodeId },
    E_DLL_LOSS_STATUSRES_TH { node_id: NodeId },
    E_DLL_CYCLE_EXCEED_TH,
    E_DLL_CYCLE_EXCEED,
    E_DLL_LATE_PRES_TH { node_id: NodeId },
    E_DLL_JITTER_TH,
    E_DLL_MS_WAIT_SOC,
    E_DLL_COLLISION,
    E_DLL_MULTIPLE_MN,
    E_DLL_ADDRESS_CONFLICT,
    E_DLL_MEV_ASND_TIMEOUT,
}


// --- 3. Counter Logic ---

/// Implements the 8:1 threshold counter logic from the specification.
/// (EPSG DS 301, Section 4.7.4.1)
#[derive(Debug, Default)]
pub struct ThresholdCounter {
    count: u32,
    threshold: u32,
}

impl ThresholdCounter {
    pub fn new(threshold: u32) -> Self {
        Self { count: 0, threshold }
    }

    /// Increment the counter by 8 when an error occurs[cite: 965].
    pub fn increment(&mut self) {
        self.count = self.count.saturating_add(8);
    }

    /// Decrement the counter by 1 for each error-free cycle[cite: 965].
    pub fn decrement(&mut self) {
        self.count = self.count.saturating_sub(1);
    }

    /// Check if the threshold has been reached. If so, reset the counter
    /// and return true[cite: 965].
    pub fn check_and_reset(&mut self) -> bool {
        if self.threshold > 0 && self.count >= self.threshold {
            self.count = 0;
            true
        } else {
            false
        }
    }
}

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
    /// Creates a new set of counters, typically with thresholds from the Object Dictionary.
    pub fn new() -> Self {
        // Default values from the specification (e.g., Table 1229)
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


// --- 4. The Central Error Manager ---

/// The central manager for handling DLL errors, generic over a logger.
pub struct DllErrorManager<H: ErrorHandler> {
    // For now, this struct is focused on the CN side.
    cn_counters: CnErrorCounters,
    handler: H,
}

impl<H: ErrorHandler> DllErrorManager<H> {
    pub fn new(handler: H) -> Self {
        Self {
            cn_counters: CnErrorCounters::new(),
            handler,
        }
    }

    /// Called when an error is detected. It updates the counters and returns an
    /// `NmtAction` if a critical threshold is met.
    pub fn handle_error(&mut self, error: DllError) -> NmtAction {
        let mut nmt_action = NmtAction::None;
        
        let threshold_reached = match error {
            DllError::E_DLL_LOSS_SOC_TH => {
                self.cn_counters.loss_of_soc.increment();
                self.cn_counters.loss_of_soc.check_and_reset()
            },
            DllError::E_DLL_CRC_TH => {
                self.cn_counters.crc_errors.increment();
                self.cn_counters.crc_errors.check_and_reset()
            },
            DllError::E_DLL_LOSS_OF_LINK => {
                self.cn_counters.loss_of_link_cumulative += 1;
                // Loss of link is logged directly without a threshold.
                self.handler.log_error(&error);
                false // Does not directly trigger an NMT state change.
            },
            // Other error cases would be handled here.
            _ => false
        };

        if threshold_reached {
            self.handler.log_error(&error);
            // Per Table 27, most threshold errors on a CN trigger a reset.
            nmt_action = NmtAction::ResetCommunication;
        }
        
        nmt_action
    }
    
    /// Must be called once per POWERLINK cycle to decrement all threshold counters.
    pub fn on_cycle_complete(&mut self) {
        self.cn_counters.loss_of_soc.decrement();
        self.cn_counters.crc_errors.decrement();
        self.cn_counters.loss_of_soa.decrement();
        self.cn_counters.loss_of_preq.decrement();
        self.cn_counters.collision.decrement();
        self.cn_counters.soc_jitter.decrement();
    }
}