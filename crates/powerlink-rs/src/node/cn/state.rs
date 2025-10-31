// crates/powerlink-rs/src/node/cn/state.rs

use crate::frame::basic::MacAddress;
use crate::frame::error::{CnErrorCounters, DllErrorManager, ErrorCounters, ErrorEntry, LoggingErrorHandler};
use crate::frame::DllCsStateMachine;
use crate::nmt::cn_state_machine::CnNmtStateMachine;
use crate::nmt::events::NmtCommand;
use crate::node::{CoreNodeContext, NodeContext, PdoHandler}; // Import CoreNodeContext
use crate::od::ObjectDictionary;
use crate::sdo::{SdoClient, SdoServer};
use crate::types::NodeId;
use crate::ErrorHandler;
use alloc::collections::VecDeque;
use alloc::vec::Vec;

/// Holds the complete state for a Controlled Node.
pub struct CnContext<'s> {
    pub core: CoreNodeContext<'s>, // Use CoreNodeContext for shared state
    pub nmt_state_machine: CnNmtStateMachine,
    pub dll_state_machine: DllCsStateMachine,
    // dll_error_manager is separated due to its generic parameters
    pub dll_error_manager: DllErrorManager<CnErrorCounters, LoggingErrorHandler>, 
    /// Queue for NMT commands this CN wants the MN to execute.
    pub pending_nmt_requests: Vec<(NmtCommand, NodeId)>,
    /// Queue for detailed error/event entries to be reported in StatusResponse.
    pub emergency_queue: VecDeque<ErrorEntry>,
    /// Timestamp of the last successfully received SoC frame (microseconds).
    pub last_soc_reception_time_us: u64,
    /// Flag indicating if the SoC timeout check is currently active.
    pub soc_timeout_check_active: bool,
    /// The absolute time in microseconds for the next scheduled tick.
    pub next_tick_us: Option<u64>,
    /// Exception New flag, toggled when new error info is available.
    pub en_flag: bool,
    /// Exception Clear flag, mirrors the last received ER flag from the MN.
    pub ec_flag: bool,
    /// A flag that is set when a new error occurs, to trigger toggling the EN flag.
    pub error_status_changed: bool,
}

// Implement the PdoHandler trait for ControlledNode
impl<'s> PdoHandler<'s> for CnContext<'s> {
    fn od(&mut self) -> &mut ObjectDictionary<'s> {
        &mut self.core.od
    }

    fn dll_error_manager(&mut self) -> &mut DllErrorManager<impl ErrorCounters, impl ErrorHandler> {
        &mut self.dll_error_manager
    }
}

impl<'s> NodeContext for CnContext<'s> {
    fn is_cn(&self) -> bool {
        true
    }
    fn core(&self) -> &CoreNodeContext {
        &self.core
    }
    fn core_mut(&mut self) -> &mut CoreNodeContext {
        &mut self.core
    }
    fn nmt_state_machine(&self) -> &dyn crate::nmt::NmtStateMachine {
        &self.nmt_state_machine
    }
}