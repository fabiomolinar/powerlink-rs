// crates/powerlink-rs/src/node/cn/state.rs
use crate::ErrorHandler;
use crate::frame::DllCsStateMachine;
use crate::frame::error::{
    CnErrorCounters, DllErrorManager, ErrorCounters, ErrorEntry, LoggingErrorHandler,
};
use crate::nmt::cn_state_machine::CnNmtStateMachine;
use crate::nmt::events::NmtCommand;
use crate::node::{CoreNodeContext, NodeContext, PdoHandler};
use crate::od::{constants, ObjectDictionary, ObjectValue}; // Import constants
use crate::pdo::{error::PdoError, PDOVersion, PdoMappingEntry}; // Import PDO types
use crate::sdo::transport::AsndTransport;
#[cfg(feature = "sdo-udp")]
use crate::sdo::transport::UdpTransport;
use crate::types::NodeId;
use crate::PowerlinkError; // Import PowerlinkError
use alloc::collections::VecDeque;
use alloc::vec; // Import vec
use alloc::vec::Vec;
use log::{error, trace, warn}; // Import log levels

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
}

/// Inherent methods for CN-specific logic, including the moved TPDO logic.
impl<'s> CnContext<'s> {
    /// Fills a buffer with the CN's TPDO payload.
    ///
    /// This implementation is for a CN, which can only have one
    /// TPDO (Comm param 0x1800, Mapping param 0x1A00) for its PRes.
    ///
    /// Returns the payload `Vec` and the `PDOVersion` for this mapping.
    /// Returns an error if the configuration is invalid.
    pub(super) fn build_tpdo_payload(
        &self,
    ) -> Result<(Vec<u8>, PDOVersion), PowerlinkError> {
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
        let mut max_offset_len = 0; // Track the highest byte written.

        // 5. Read the number of mapped objects from 0x1A00/0.
        if let Some(ObjectValue::Unsigned8(num_entries)) =
            self.core.od.read(mapping_index, 0).as_deref()
        {
            if *num_entries > 0 {
                trace!(
                    "Building TPDO payload using {:#06X} with {} entries.",
                    mapping_index,
                    num_entries
                );
                // 6. Iterate through each mapping entry.
                for i in 1..=*num_entries {
                    if let Some(ObjectValue::Unsigned64(raw_mapping)) =
                        self.core.od.read(mapping_index, i).as_deref()
                    {
                        let entry = PdoMappingEntry::from_u64(*raw_mapping);
                        if let Err(e) = self.apply_tpdo_mapping_entry(&entry, &mut payload) {
                            // On error (e.g., buffer too small, type mismatch),
                            // we must stop and return an error.
                            error!("[PDO] Failed to apply TPDO mapping entry for {:#06X}/{}: {:?}. Invalidating TPDO.", entry.index, entry.sub_index, e);
                            return Err(e.into());
                        }
                        // Track the max byte written to truncate later if needed
                        // (though PRes payload is fixed size)
                        max_offset_len = max_offset_len.max(
                            entry.byte_offset().unwrap_or(0) + entry.byte_length().unwrap_or(0),
                        );
                    } else {
                        warn!("[CN] Mapping entry {} for TPDO (PRes) is not U64", i);
                    }
                }
            }
        } else {
            warn!(
                "[CN] TPDO Mapping object {:#06X} not found or is invalid.",
                mapping_index
            );
        }

        // The payload size is fixed by the limit. Do not truncate.
        trace!("[CN] Built PRes payload with fixed size: {}", payload.len());
        Ok((payload, pdo_version))
    }

    /// Helper for `build_tpdo_payload` to apply a single mapping entry.
    /// Returns a Result to indicate if processing should stop.
    fn apply_tpdo_mapping_entry(
        &self,
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

        // Read the value from the OD
        let Some(value) = self.core.od.read(entry.index, entry.sub_index) else {
            warn!(
                "TPDO mapping for 0x{:04X}/{} failed: OD entry not found. Filling with zeros.",
                entry.index, entry.sub_index
            );
            // The buffer is already zero-filled, so just return Ok.
            return Ok(());
        };

        // Serialize the value into the slice
        let bytes_to_pack = value.serialize();

        if bytes_to_pack.len() != length {
            // Error: The actual data size from the OD does not match
            // the length specified in the mapping
            warn!(
                "TPDO serialize mismatch for 0x{:04X}/{}: mapping length is {} bytes, but value serialized {} bytes.",
                entry.index, entry.sub_index, length, bytes_to_pack.len()
            );
            return Err(PdoError::TypeMismatch {
                index: entry.index,
                sub_index: entry.sub_index,
                expected_bits: length as u16 * 8,
                actual_bits: (bytes_to_pack.len() * 8) as u16,
            });
        }

        // Copy the serialized bytes into the payload buffer slice
        data_slice.copy_from_slice(&bytes_to_pack);

        Ok(())
    }
}