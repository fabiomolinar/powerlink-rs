// crates/powerlink-rs/src/node/cn/state.rs
use crate::ErrorHandler;
use crate::PowerlinkError;
use crate::common::{NetTime, RelativeTime}; // Added RelativeTime
use crate::frame::DllCsStateMachine;
use crate::frame::error::{
    CnErrorCounters, DllErrorManager, ErrorCounters, ErrorEntry, LoggingErrorHandler,
};
use crate::nmt::cn_state_machine::CnNmtStateMachine;
use crate::nmt::events::{CnNmtRequest, NmtServiceRequest};
use crate::node::{CoreNodeContext, NodeContext, PdoHandler};
use crate::od::{ObjectValue, constants};
use crate::pdo::{PDOVersion, PdoMappingEntry, error::PdoError};
use crate::sdo::transport::AsndTransport;
#[cfg(feature = "sdo-udp")]
use crate::sdo::transport::UdpTransport;
use crate::types::NodeId;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::vec;
use alloc::vec::Vec;
use log::{error, info, trace, warn};

/// Holds the complete state for a Controlled Node.
pub struct CnContext<'s> {
    pub core: CoreNodeContext<'s>, // Use CoreNodeContext for shared state
    pub nmt_state_machine: CnNmtStateMachine,
    pub dll_state_machine: DllCsStateMachine,
    // dll_error_manager is separated due to its generic parameters
    pub dll_error_manager: DllErrorManager<CnErrorCounters, LoggingErrorHandler>,
    /// SDO transport handler for ASnd.
    pub asnd_transport: AsndTransport,
    /// SDO transport handler for UDP.
    #[cfg(feature = "sdo-udp")]
    pub udp_transport: UdpTransport,
    
    // --- Synchronization Hooks (Added in Step 4) ---
    /// The NetTime extracted from the last received SoC frame.
    pub last_soc_net_time: NetTime,
    /// The RelativeTime extracted from the last received SoC frame.
    pub last_soc_relative_time: RelativeTime,
    /// The local monotonic timestamp (in microseconds) when the last SoC was received.
    pub last_soc_arrival_time_us: u64,
    // -----------------------------------------------

    /// Queue for NMT commands this CN wants the MN to execute.
    pub pending_nmt_requests: Vec<(CnNmtRequest, NodeId)>,
    /// Queue for detailed error/event entries to be reported in StatusResponse.
    pub emergency_queue: VecDeque<ErrorEntry>,
    /// Map of nodes to monitor via heartbeat, mapping NodeId -> (Timeout in us, LastSeen time in us).
    pub heartbeat_consumers: BTreeMap<NodeId, (u64, u64)>,
    /// Timestamp of the last successfully received SoC frame (microseconds).
    /// Used for timeout monitoring.
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

impl<'s> CnContext<'s> {
    /// Helper to get the time elapsed since the start of the current cycle.
    pub fn time_since_soc(&self, current_time_us: u64) -> u64 {
        current_time_us.saturating_sub(self.last_soc_arrival_time_us)
    }

    /// Internal helper to queue an NMT service request.
    pub(super) fn queue_nmt_service_request(&mut self, service: NmtServiceRequest, target: NodeId) {
        info!(
            "Queueing NMT Service request: Service={:?}, Target={}",
            service, target.0
        );
        self.pending_nmt_requests
            .push((CnNmtRequest::Service(service), target));
    }

    /// Fills a buffer with the CN's TPDO payload.
    pub(super) fn build_tpdo_payload(&mut self) -> Result<(Vec<u8>, PDOVersion), PowerlinkError> {
        // 1. Get the TPDO mapping (1A00h for a CN's PRes).
        let mapping_index = constants::IDX_TPDO_MAPPING_PARAM_REC_START; // 0x1A00
        let comm_param_index = constants::IDX_TPDO_COMM_PARAM_REC_START; // 0x1800

        // 2. Get Mapping Version from 0x1800/2
        let pdo_version = PDOVersion(
            self.core
                .od
                .read_u8(
                    comm_param_index,
                    constants::SUBIDX_PDO_COMM_PARAM_VERSION_U8,
                )
                .unwrap_or(0),
        );

        // 3. Get the configured payload size limit for this PRes from 0x1F98/5.
        let payload_limit = self
            .core
            .od
            .read_u16(
                constants::IDX_NMT_CYCLE_TIMING_REC,
                constants::SUBIDX_NMT_CYCLE_TIMING_PRES_ACT_PAYLOAD_U16,
            )
            .unwrap_or(36) as usize;

        // Clamp to the absolute maximum allowed by the specification.
        let payload_limit = payload_limit.min(crate::types::C_DLL_ISOCHR_MAX_PAYL as usize);

        // 4. Pre-allocate a buffer of the fixed payload size.
        let mut payload = vec![0u8; payload_limit];

        // 5. Read the number of mapped objects from 0x1A00/0.
        let num_entries = self
            .core
            .od
            .read(mapping_index, 0)
            .and_then(|cow| match *cow {
                ObjectValue::Unsigned8(num) => Some(num),
                _ => None,
            })
            .unwrap_or(0);

        if num_entries > 0 {
            trace!(
                "Building TPDO payload using {:#06X} with {} entries.",
                mapping_index, num_entries
            );
            // 6. Iterate through each mapping entry.
            let mut mapping_entries = Vec::new();
            for i in 1..=num_entries {
                if let Some(ObjectValue::Unsigned64(raw_mapping)) =
                    self.core.od.read(mapping_index, i).as_deref()
                {
                    mapping_entries.push(PdoMappingEntry::from_u64(*raw_mapping));
                } else {
                    warn!("[CN] Mapping entry {} for TPDO (PRes) is not U64", i);
                    mapping_entries.push(PdoMappingEntry::from_u64(0));
                }
            }

            for entry in &mapping_entries {
                if let Err(e) = self.apply_tpdo_mapping_entry(entry, &mut payload) {
                    error!(
                        "[PDO] Failed to apply TPDO mapping entry for {:#06X}/{}: {:?}. Invalidating TPDO.",
                        entry.index, entry.sub_index, e
                    );
                    return Err(e.into());
                }
            }
        } else {
            warn!(
                "[CN] TPDO Mapping object {:#06X} not found or is invalid.",
                mapping_index
            );
        }

        Ok((payload, pdo_version))
    }

    /// Helper for `build_tpdo_payload` to apply a single mapping entry.
    fn apply_tpdo_mapping_entry(
        &mut self,
        entry: &PdoMappingEntry,
        payload_buffer: &mut [u8],
    ) -> Result<(), PdoError> {
        let (Some(offset), Some(length)) = (entry.byte_offset(), entry.byte_length()) else {
            warn!(
                "Bit-level TPDO mapping is not supported. Index: 0x{:04X}, SubIndex: {}.",
                entry.index, entry.sub_index
            );
            return Ok(()); // Continue with next entry
        };

        if payload_buffer.len() < offset + length {
            warn!(
                "TPDO mapping for 0x{:04X}/{} is out of bounds. Buffer size: {}, expected at least {}.",
                entry.index,
                entry.sub_index,
                payload_buffer.len(),
                offset + length
            );
            return Err(PdoError::PayloadTooSmall {
                expected_bits: (offset + length) as u16 * 8,
                actual_bytes: payload_buffer.len(),
            });
        }

        let data_slice = &mut payload_buffer[offset..offset + length];

        // --- SDO-in-PDO LOGIC ---
        match entry.index {
            0x1200..=0x127F => {
                trace!(
                    "[SDO-PDO] Server: Building response for TPDO channel {:#06X}",
                    entry.index
                );
                let response_payload = self
                    .core
                    .embedded_sdo_server
                    .get_pending_response(entry.index, length);
                data_slice.copy_from_slice(&response_payload);
                return Ok(());
            }
            0x1280..=0x12FF => {
                trace!(
                    "[SDO-PDO] Client: Building request for TPDO channel {:#06X}",
                    entry.index
                );
                let request_payload = self
                    .core
                    .embedded_sdo_client
                    .get_pending_request(entry.index, length);
                data_slice.copy_from_slice(&request_payload);
                return Ok(());
            }
            _ => {}
        }

        // Read the value from the OD
        let Some(value) = self.core.od.read(entry.index, entry.sub_index) else {
            warn!(
                "TPDO mapping for 0x{:04X}/{} failed: OD entry not found. Filling with zeros.",
                entry.index, entry.sub_index
            );
            return Ok(());
        };

        let bytes_to_pack = value.serialize();

        if bytes_to_pack.len() != length {
            warn!(
                "TPDO serialize mismatch for 0x{:04X}/{}: mapping length is {} bytes, but value serialized {} bytes.",
                entry.index,
                entry.sub_index,
                length,
                bytes_to_pack.len()
            );
            return Err(PdoError::TypeMismatch {
                index: entry.index,
                sub_index: entry.sub_index,
                expected_bits: length as u16 * 8,
                actual_bits: (bytes_to_pack.len() * 8) as u16,
            });
        }

        data_slice.copy_from_slice(&bytes_to_pack);
        Ok(())
    }
}

impl<'s> PdoHandler<'s> for CnContext<'s> {
    fn dll_error_manager(&mut self) -> &mut DllErrorManager<impl ErrorCounters, impl ErrorHandler> {
        &mut self.dll_error_manager
    }
}

impl<'s> NodeContext<'s> for CnContext<'s> {
    fn is_cn(&self) -> bool {
        true
    }
    fn core(&self) -> &CoreNodeContext<'s> {
        &self.core
    }
    fn core_mut(&mut self) -> &mut CoreNodeContext<'s> {
        &mut self.core
    }
    fn nmt_state_machine(&self) -> &dyn crate::nmt::NmtStateMachine {
        &self.nmt_state_machine
    }
    fn node_id(&self) -> NodeId {
        self.core.node_id
    }
}