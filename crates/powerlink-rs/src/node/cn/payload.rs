// crates/powerlink-rs/src/node/cn/payload.rs

use crate::frame::basic::MacAddress;
#[cfg(feature = "sdo-udp")]
use crate::types::IpAddress;
use crate::{od::ObjectDictionary, types::NodeId, PowerlinkError};
use crate::frame::error::ErrorEntry;
use crate::frame::poll::{PResFlags, RSFlag};
use crate::frame::{ASndFrame, PResFrame, PowerlinkFrame, ServiceId};
use crate::nmt::events::NmtCommand;
use crate::nmt::states::NmtState;
use crate::od::ObjectValue;
use crate::pdo::{PDOVersion, PdoMappingEntry};
use crate::sdo::SdoServer;
use crate::sdo::command::{CommandId, CommandLayerHeader, SdoCommand, Segmentation};
use crate::sdo::sequence::{ReceiveConnState, SendConnState, SequenceLayerHeader};
use crate::types::{C_ADR_MN_DEF_NODE_ID};
use alloc::collections::VecDeque;
use alloc::vec;
use alloc::vec::Vec;
use log::{debug, error, info, trace, warn};

// Constants for OD access
const OD_IDX_TPDO_COMM_PARAM_BASE: u16 = 0x1800;
const OD_IDX_TPDO_MAPP_PARAM_BASE: u16 = 0x1A00;
const OD_IDX_CYCLE_TIMING_REC: u16 = 0x1F98; // NMT_CycleTiming_REC
const OD_SUBIDX_PRES_PAYLOAD_LIMIT: u8 = 5; // PresActPayloadLimit_U16 in 0x1F98
const OD_SUBIDX_PDO_COMM_VERSION: u8 = 2;
const OD_IDX_ERROR_REGISTER: u16 = 0x1001;
const OD_SUBIDX_ASYNC_MTU: u8 = 8; // AsyncMTU_U16 in 0x1F98

/// Builds an `ASnd` frame for the `IdentResponse` service.
pub(super) fn build_ident_response(
    mac_address: MacAddress,
    node_id: NodeId,
    od: &ObjectDictionary,
    soa: &crate::frame::SoAFrame,
) -> PowerlinkFrame {
    debug!("Building IdentResponse for SoA from node {}", soa.source.0);
    let payload = build_ident_response_payload(od);
    let asnd = ASndFrame::new(
        mac_address,
        soa.eth_header.source_mac,    // Send back to the MN's MAC
        NodeId(C_ADR_MN_DEF_NODE_ID), // Destination Node ID is MN
        node_id,
        ServiceId::IdentResponse,
        payload,
    );
    PowerlinkFrame::ASnd(asnd)
}

/// Builds an `ASnd` frame for the `StatusResponse` service.
pub(super) fn build_status_response(
    mac_address: MacAddress,
    node_id: NodeId,
    od: &mut ObjectDictionary,
    en_flag: bool,
    ec_flag: bool,
    emergency_queue: &mut VecDeque<ErrorEntry>,
    soa: &crate::frame::SoAFrame,
) -> PowerlinkFrame {
    debug!("Building StatusResponse for SoA from node {}", soa.source.0);
    let payload = build_status_response_payload(od, en_flag, ec_flag, emergency_queue);
    let asnd = ASndFrame::new(
        mac_address,
        soa.eth_header.source_mac,    // Send back to MN's MAC
        NodeId(C_ADR_MN_DEF_NODE_ID), // Destination is MN
        node_id,
        ServiceId::StatusResponse,
        payload,
    );
    PowerlinkFrame::ASnd(asnd)
}

/// Builds an `ASnd` frame for the `NMTRequest` service.
pub(super) fn build_nmt_request(
    mac_address: MacAddress,
    node_id: NodeId,
    command: NmtCommand,
    target: NodeId,
    soa: &crate::frame::SoAFrame,
) -> PowerlinkFrame {
    debug!(
        "Building NMTRequest(Command={:?}, Target={}) for SoA from node {}",
        command, target.0, soa.source.0
    );
    // Payload format from Spec Table 144
    let payload = vec![
        command as u8, // NMTRequestedCommandID
        target.0,      // NMTRequestedCommandTarget
                       // NMTRequestedCommandData is omitted for plain commands
    ];

    let asnd = ASndFrame::new(
        mac_address,
        soa.eth_header.source_mac,
        NodeId(C_ADR_MN_DEF_NODE_ID),
        node_id,
        ServiceId::NmtRequest,
        payload,
    );
    PowerlinkFrame::ASnd(asnd)
}

/// Builds a `PRes` frame in response to being polled by a `PReq`.
pub(super) fn build_pres_response(
    mac_address: MacAddress,
    node_id: NodeId,
    nmt_state: NmtState,
    od: &ObjectDictionary,
    sdo_server: &SdoServer,
    pending_nmt_requests: &[(NmtCommand, NodeId)], // Add NMT request queue
    en_flag: bool,
) -> PowerlinkFrame {
    debug!("Building PRes in response to PReq for node {}", node_id.0);

    let (payload, pdo_version, payload_is_valid) = match build_tpdo_payload(od) {
        Ok((payload, version)) => (payload, version, true),
        Err(e) => {
            error!( // Changed to error as this indicates a configuration problem
                "Failed to build TPDO payload for PRes: {:?}. Sending empty PRes.",
                e
            );
            (Vec::new(), PDOVersion(0), false)
        }
    };

    // RD flag is only set if NMT is operational AND the payload was built successfully
    let rd_flag = (nmt_state == NmtState::NmtOperational) && payload_is_valid;

    // Check for pending SDO/NMT requests to set RS and PR flags.
    // NMT requests (PR=7) have higher priority than generic SDO requests (PR=3).
    let (rs_count, pr_flag) = if !pending_nmt_requests.is_empty() {
        (
            pending_nmt_requests.len().min(7) as u8,
            crate::frame::PRFlag::PrioNmtRequest,
        )
    } else {
        sdo_server.pending_request_count_and_priority()
    };

    let flags = PResFlags {
        rd: rd_flag,
        en: en_flag,
        rs: RSFlag::new(rs_count),
        pr: pr_flag,
        ..Default::default() // Let ms be false by default
    };

    let pres = PResFrame::new(mac_address, node_id, nmt_state, flags, pdo_version, payload);
    PowerlinkFrame::PRes(pres)
}


/// Helper to serialize SDO Sequence + Command into a buffer suitable for ASnd payload.
pub(super) fn serialize_sdo_asnd_payload(
    seq_header: SequenceLayerHeader,
    cmd: SdoCommand,
) -> Result<Vec<u8>, PowerlinkError> {
    // Allocate buffer based on command payload size + headers
    let estimated_size = 4 // Sequence Header
                       + 4 // Command Header Fixed Part
                       + if cmd.data_size.is_some() { 4 } else { 0 } // Optional Data Size
                       + cmd.payload.len();
    // Use Vec directly instead of pre-allocating large buffer
    let mut buffer = Vec::with_capacity(estimated_size);

    // Serialize Sequence Layer Header (4 bytes)
    let mut seq_buf = [0u8; 4];
    seq_header.serialize(&mut seq_buf)?;
    buffer.extend_from_slice(&seq_buf);

    // Serialize Command Layer (Header + Payload)
    // Need a temporary buffer for cmd.serialize as it writes into a slice
    let mut cmd_buf = vec![0u8; estimated_size - 4]; // Max possible size for cmd part
    let cmd_len = cmd.serialize(&mut cmd_buf)?;
    buffer.extend_from_slice(&cmd_buf[..cmd_len]); // Append only the bytes written

    Ok(buffer)
}

/// Constructs the detailed payload for an `IdentResponse` frame by reading from the OD.
/// The structure is defined in EPSG DS 301, Section 7.3.3.2.1.
fn build_ident_response_payload(od: &ObjectDictionary) -> Vec<u8> {
    // Size according to spec: 158 bytes total payload
    let mut payload = vec![0u8; 158];

    // --- Populate fields based on OD values ---

    // Flags (Octet 0-1): PR/RS - Assume none pending for now (needs SdoServer access if implemented)
    // NMTState (Octet 2)
    payload[2] = od.read_u8(0x1F8C, 0).unwrap_or(0); // Read NMT_CurrNMTState_U8
    // Reserved (Octet 3)
    // EPLVersion (Octet 4) - from 0x1F83/0
    payload[4] = od.read_u8(0x1F83, 0).unwrap_or(0);
    // Reserved (Octet 5)
    // FeatureFlags (Octets 6-9) - from 0x1F82/0
    payload[6..10].copy_from_slice(&od.read_u32(0x1F82, 0).unwrap_or(0).to_le_bytes());
    // MTU (Octets 10-11) - from 0x1F98/8 (AsyncMTU_U16)
    payload[10..12].copy_from_slice(&od.read_u16(0x1F98, 8).unwrap_or(0).to_le_bytes());
    // PollInSize (Octets 12-13) - from 0x1F98/4 (PreqActPayloadLimit_U16)
    payload[12..14].copy_from_slice(&od.read_u16(0x1F98, 4).unwrap_or(0).to_le_bytes());
    // PollOutSize (Octets 14-15) - from 0x1F98/5 (PresActPayloadLimit_U16)
    payload[14..16].copy_from_slice(&od.read_u16(0x1F98, 5).unwrap_or(0).to_le_bytes());
    // ResponseTime (Octets 16-19) - from 0x1F98/3 (PresMaxLatency_U32)
    payload[16..20].copy_from_slice(&od.read_u32(0x1F98, 3).unwrap_or(0).to_le_bytes());
    // Reserved (Octets 20-21)
    // DeviceType (Octets 22-25) - from 0x1000/0
    payload[22..26].copy_from_slice(&od.read_u32(0x1000, 0).unwrap_or(0).to_le_bytes());
    // VendorID (Octets 26-29) - from 0x1018/1
    payload[26..30].copy_from_slice(&od.read_u32(0x1018, 1).unwrap_or(0).to_le_bytes());
    // ProductCode (Octets 30-33) - from 0x1018/2
    payload[30..34].copy_from_slice(&od.read_u32(0x1018, 2).unwrap_or(0).to_le_bytes());
    // RevisionNumber (Octets 34-37) - from 0x1018/3
    payload[34..38].copy_from_slice(&od.read_u32(0x1018, 3).unwrap_or(0).to_le_bytes());
    // SerialNumber (Octets 38-41) - from 0x1018/4
    payload[38..42].copy_from_slice(&od.read_u32(0x1018, 4).unwrap_or(0).to_le_bytes());
    // VendorSpecificExtension1 (Octets 42-49) - Skipped (zeros)
    // VerifyConfigurationDate (Octets 50-53) - from 0x1020/1
    payload[50..54].copy_from_slice(&od.read_u32(0x1020, 1).unwrap_or(0).to_le_bytes());
    // VerifyConfigurationTime (Octets 54-57) - from 0x1020/2
    payload[54..58].copy_from_slice(&od.read_u32(0x1020, 2).unwrap_or(0).to_le_bytes());
    // ApplicationSwDate (Octets 58-61) - from 0x1F52/1
    payload[58..62].copy_from_slice(&od.read_u32(0x1F52, 1).unwrap_or(0).to_le_bytes());
    // ApplicationSwTime (Octets 62-65) - from 0x1F52/2
    payload[62..66].copy_from_slice(&od.read_u32(0x1F52, 2).unwrap_or(0).to_le_bytes());
    // IPAddress (Octets 66-69) - from 0x1E40/2
    payload[66..70].copy_from_slice(&od.read_u32(0x1E40, 2).unwrap_or(0).to_le_bytes());
    // SubnetMask (Octets 70-73) - from 0x1E40/3
    payload[70..74].copy_from_slice(&od.read_u32(0x1E40, 3).unwrap_or(0).to_le_bytes());
    // DefaultGateway (Octets 74-77) - from 0x1E40/5
    payload[74..78].copy_from_slice(&od.read_u32(0x1E40, 5).unwrap_or(0).to_le_bytes());
    // HostName (Octets 78-109) - from 0x1F9A/0 (VISIBLE_STRING32)
    if let Some(cow_val) = od.read(0x1F9A, 0) {
        if let crate::od::ObjectValue::VisibleString(s) = &*cow_val {
            let bytes = s.as_bytes();
            let len = bytes.len().min(32); // Max 32 bytes for hostname field
            payload[78..78 + len].copy_from_slice(&bytes[..len]);
        }
    }
    // VendorSpecificExtension2 (Octets 110-157) - Skipped (zeros)

    payload
}

/// Builds the payload for a `StatusResponse` frame.
/// The structure is defined in EPSG DS 301, Section 7.3.3.3.1.
fn build_status_response_payload(
    od: &mut ObjectDictionary,
    en_flag: bool,
    ec_flag: bool,
    emergency_queue: &mut VecDeque<ErrorEntry>,
) -> Vec<u8> {
    // --- Determine max payload size ---
    // The payload limit is defined by AsyncMTU (0x1F98/8), minus ASnd header (4 bytes)
    let mtu = od.read_u16(OD_IDX_CYCLE_TIMING_REC, OD_SUBIDX_ASYNC_MTU).unwrap_or(300) as usize;
    let max_payload_size = mtu.saturating_sub(4);

    // --- Base payload with fixed fields ---
    let mut payload = vec![0u8; 14];
    // Octet 0: Flags (EN, EC)
    if en_flag { payload[0] |= 1 << 5; }
    if ec_flag { payload[0] |= 1 << 4; }
    // Octet 1: Flags (PR, RS) - TODO: Needs access to SdoServer state
    // Octet 2: NMTState
    payload[2] = od.read_u8(0x1F8C, 0).unwrap_or(0);
    // Octets 3-5: Reserved
    // Octets 6-13: StaticErrorBitField
    payload[6] = od.read_u8(OD_IDX_ERROR_REGISTER, 0).unwrap_or(0);
    // Bytes 7-13 are reserved or device specific errors. Keep as zero for now.

    // --- Append Error/Event History Entries from emergency queue ---
    let mut entry_buffer = [0u8; 20]; // Buffer to serialize one entry
    while let Some(entry) = emergency_queue.front() {
        if payload.len() + 20 > max_payload_size {
            warn!("[CN] Not enough space in StatusResponse for all queued errors. Remaining: {}", emergency_queue.len());
            break;
        }

        entry_buffer.fill(0);
        entry_buffer[0..2].copy_from_slice(&((entry.entry_type.profile & 0x0FFF) | ((entry.entry_type.mode as u16) << 12) | if entry.entry_type.is_status_entry { 1 << 15 } else { 0 } | if entry.entry_type.send_to_queue { 1 << 14 } else { 0 }).to_le_bytes());
        entry_buffer[2..4].copy_from_slice(&entry.error_code.to_le_bytes());
        entry_buffer[4..8].copy_from_slice(&entry.timestamp.seconds.to_le_bytes());
        entry_buffer[8..12].copy_from_slice(&entry.timestamp.nanoseconds.to_le_bytes());
        entry_buffer[12..20].copy_from_slice(&entry.additional_information.to_le_bytes());

        payload.extend_from_slice(&entry_buffer);
        emergency_queue.pop_front();
        trace!("[CN] Added error entry to StatusResponse. Queue size: {}", emergency_queue.len());
    }

    if payload.len() + 20 <= max_payload_size {
        payload.extend_from_slice(&[0u8; 20]); // Add terminator entry
    }

    payload
}

/// Builds the payload for a TPDO (PRes) frame.
/// Uses mapping 0x1A00 and respects payload limit 0x1F98/5.
fn build_tpdo_payload(od: &ObjectDictionary) -> Result<(Vec<u8>, PDOVersion), PowerlinkError> {
    let comm_param_index = OD_IDX_TPDO_COMM_PARAM_BASE; // Default PRes is 0x1800
    let mapping_index = OD_IDX_TPDO_MAPP_PARAM_BASE; // Default PRes is 0x1A00

    // 1. Get Mapping Version from 0x1800/2
    let pdo_version = PDOVersion(
        od.read_u8(comm_param_index, OD_SUBIDX_PDO_COMM_VERSION)
            .unwrap_or(0),
    );

    // 2. Get Payload Limit from 0x1F98/5 (PresActPayloadLimit_U16)
    let payload_limit = od
        .read_u16(OD_IDX_CYCLE_TIMING_REC, OD_SUBIDX_PRES_PAYLOAD_LIMIT)
        .unwrap_or(36) as usize; // Default to 36 if not found

    // Clamp payload_limit to absolute maximum
    let payload_limit = payload_limit.min(crate::types::C_DLL_ISOCHR_MAX_PAYL as usize);

    // Pre-allocate buffer based on the limit
    let mut payload = vec![0u8; payload_limit];
    let mut max_offset_len = 0; // Track the actual highest byte written

    // 3. Iterate mapping entries from 0x1A00
    if let Some(mapping_cow) = od.read(mapping_index, 0) {
        if let ObjectValue::Unsigned8(num_entries) = *mapping_cow {
             if num_entries > 0 { // Only proceed if mapping is enabled
                trace!("Building TPDO payload using {:#06X} with {} entries.", mapping_index, num_entries);
                for i in 1..=num_entries {
                    if let Some(entry_cow) = od.read(mapping_index, i) {
                        if let ObjectValue::Unsigned64(raw_mapping) = *entry_cow {
                            let entry = PdoMappingEntry::from_u64(raw_mapping);

                            let (Some(offset), Some(length)) = (entry.byte_offset(), entry.byte_length())
                            else {
                                warn!(
                                    "[CN] Bit-level TPDO mapping not supported for 0x{:04X}/{}",
                                    entry.index, entry.sub_index
                                );
                                continue;
                            };

                            let end_pos = offset + length;

                            if end_pos > payload_limit {
                                error!(
                                    "[CN] TPDO mapping for 0x{:04X}/{} (offset {}, len {}) exceeds PRes payload limit {} bytes. Mapping invalid. [E_PDO_MAP_OVERRUN]",
                                    entry.index, entry.sub_index, offset, length, payload_limit
                                );
                                return Err(PowerlinkError::ValidationError(
                                    "PDO mapping exceeds PRes payload limit",
                                ));
                            }
                            max_offset_len = max_offset_len.max(end_pos);

                            let Some(value_cow) = od.read(entry.index, entry.sub_index) else {
                                warn!(
                                    "[CN] TPDO mapping for 0x{:04X}/{} failed: OD entry not found. Filling with zeros.",
                                    entry.index, entry.sub_index
                                );
                                payload[offset..end_pos].fill(0);
                                continue;
                            };

                            let serialized_data = value_cow.serialize();
                            if serialized_data.len() != length {
                                warn!(
                                    "[CN] TPDO mapping for 0x{:04X}/{} length mismatch. Mapped: {} bytes, Object: {} bytes. Truncating/Padding.",
                                    entry.index,
                                    entry.sub_index,
                                    length,
                                    serialized_data.len()
                                );
                                let copy_len = serialized_data.len().min(length);
                                payload[offset..offset + copy_len]
                                    .copy_from_slice(&serialized_data[..copy_len]);
                                if length > copy_len {
                                    payload[offset + copy_len..end_pos].fill(0);
                                }
                            } else {
                                payload[offset..end_pos].copy_from_slice(&serialized_data);
                            }
                            trace!(
                                "[CN] Applied TPDO to PRes: Read {:?} from 0x{:04X}/{}",
                                value_cow, entry.index, entry.sub_index
                            );
                        } else { warn!( "[CN] Mapping entry {} for TPDO (PRes) is not U64", i ); }
                    } else { warn!( "[CN] Could not read mapping entry {} for TPDO (PRes)", i ); }
                }
            } else { trace!("[CN] TPDO Mapping {:#06X} is disabled (0 entries).", mapping_index); }
        } else { warn!( "[CN] TPDO Mapping object {:#06X} sub-index 0 not found or not U8.", mapping_index ); }
    } else { warn!("[CN] TPDO Mapping object {:#06X} not found.", mapping_index); }

    payload.truncate(max_offset_len);
    trace!(
        "[CN] Built PRes payload with actual size: {}",
        payload.len()
    );
    Ok((payload, pdo_version))
}

