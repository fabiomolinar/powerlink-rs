use crate::frame::basic::MacAddress;
use crate::frame::poll::PResFlags;
use crate::frame::{ASndFrame, PResFrame, PowerlinkFrame, ServiceId};
use crate::nmt::states::NmtState;
use crate::od::{ObjectDictionary, ObjectValue};
use crate::pdo::{PDOVersion, PdoMappingEntry};
use crate::sdo::SdoServer;
use crate::sdo::command::{CommandId, CommandLayerHeader, SdoCommand, Segmentation};
use crate::sdo::sequence::{ReceiveConnState, SendConnState, SequenceLayerHeader};
use crate::types::{C_ADR_MN_DEF_NODE_ID, NodeId};
use crate::PowerlinkError;
use alloc::vec;
use alloc::vec::Vec;
use log::{debug, error, trace, warn};

// Constants for OD access
const OD_IDX_TPDO_COMM_PARAM_BASE: u16 = 0x1800;
const OD_IDX_TPDO_MAPP_PARAM_BASE: u16 = 0x1A00;
const OD_IDX_CYCLE_TIMING_REC: u16 = 0x1F98; // NMT_CycleTiming_REC
const OD_SUBIDX_PRES_PAYLOAD_LIMIT: u8 = 5; // PresActPayloadLimit_U16 in 0x1F98
const OD_SUBIDX_PDO_COMM_VERSION: u8 = 2;

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

/// Builds a `PRes` frame in response to being polled by a `PReq`.
pub(super) fn build_pres_response(
    mac_address: MacAddress,
    node_id: NodeId,
    nmt_state: NmtState,
    od: &ObjectDictionary, // Changed to immutable reference for build_tpdo_payload
) -> PowerlinkFrame {
    debug!("Building PRes in response to PReq for node {}", node_id.0);

    let (payload, pdo_version, payload_is_valid) = match build_tpdo_payload(od) {
        Ok((payload, version)) => (payload, version, true),
        Err(e) => {
            warn!("Failed to build TPDO payload for PRes: {:?}. Sending empty PRes.", e);
            (Vec::new(), PDOVersion(0), false)
        }
    };

    // RD flag is only set if NMT is operational AND the payload was built successfully
    let rd_flag = (nmt_state == NmtState::NmtOperational) && payload_is_valid;

    // TODO: Determine RS and PR flags from SDO/NMT queues
    let flags = PResFlags {
        rd: rd_flag,
        ..Default::default()
    };

    let pres = PResFrame::new(mac_address, node_id, nmt_state, flags, pdo_version, payload);
    PowerlinkFrame::PRes(pres)
}

/// Constructs the detailed payload for an `IdentResponse` frame by reading from the OD.
/// The structure is defined in EPSG DS 301, Section 7.3.3.2.1.
fn build_ident_response_payload(od: &ObjectDictionary) -> Vec<u8> {
    // Size according to spec: 158 bytes total payload
    let mut payload = vec![0u8; 158];

    // --- Populate fields based on OD values ---

    // Flags (Octet 0-1): PR/RS - Assume none pending for now
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

/// Builds an ASnd frame containing an SDO Abort message.
pub(super) fn build_sdo_abort_response(
    mac_address: MacAddress,
    node_id: NodeId,
    sdo_server: &SdoServer,
    transaction_id: u8,
    abort_code: u32,
    client_node_id: NodeId,
    client_mac: MacAddress,
) -> PowerlinkFrame {
    error!(
        "Building SDO Abort response (TID: {}, Code: {:#010X}) for Node {}",
        transaction_id, abort_code, client_node_id.0
    );

    // Construct the SDO Abort command
    let abort_command = SdoCommand {
        header: CommandLayerHeader {
            transaction_id,
            is_response: true,
            is_aborted: true,
            segmentation: Segmentation::Expedited,
            command_id: CommandId::Nil, // Command ID irrelevant for abort
            segment_size: 4,            // Size of the abort code payload
        },
        data_size: None,
        payload: abort_code.to_le_bytes().to_vec(),
    };

    // Construct the Sequence Layer header (state remains Established during abort)
    let seq_header = SequenceLayerHeader {
        receive_sequence_number: sdo_server.current_receive_sequence(), // Ack last received
        receive_con: ReceiveConnState::ConnectionValid,
        send_sequence_number: sdo_server.next_send_sequence(), // Use next send number
        send_con: SendConnState::ConnectionValid,
    };

    // Serialize SDO Seq + Cmd into SDO payload buffer
    let mut sdo_payload_buf = vec![0u8; 12]; // 4 (Seq) + 8 (Cmd fixed) + 4 (Abort Code)
    seq_header
        .serialize(&mut sdo_payload_buf[0..4])
        .unwrap_or_else(|e| {
            error!("Failed to serialize SDO Seq header for abort: {:?}", e);
            0 // Should not fail with correct buffer size
        });
    abort_command
        .serialize(&mut sdo_payload_buf[4..])
        .unwrap_or_else(|e| {
            error!("Failed to serialize SDO Cmd header for abort: {:?}", e);
            0 // Should not fail
        });

    // Construct the ASnd frame
    let abort_asnd = ASndFrame::new(
        mac_address,
        client_mac,
        client_node_id,
        node_id,
        ServiceId::Sdo,
        sdo_payload_buf,
    );
    warn!("Sending SDO Abort frame.");
    PowerlinkFrame::ASnd(abort_asnd)
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
            for i in 1..=num_entries {
                let Some(entry_cow) = od.read(mapping_index, i) else {
                    warn!("[CN] Could not read mapping entry {} for TPDO (PRes)", i);
                    continue; // Skip this entry
                };
                let ObjectValue::Unsigned64(raw_mapping) = *entry_cow else {
                    warn!("[CN] Mapping entry {} for TPDO (PRes) is not U64", i);
                    continue; // Skip this entry
                };

                let entry = PdoMappingEntry::from_u64(raw_mapping);

                // Assuming byte alignment for now
                let (Some(offset), Some(length)) = (entry.byte_offset(), entry.byte_length()) else {
                    warn!(
                        "[CN] Bit-level TPDO mapping not supported for 0x{:04X}/{}",
                        entry.index, entry.sub_index
                    );
                    continue; // Skip this entry
                };

                let end_pos = offset + length;

                // Check if mapping exceeds the payload limit defined in 0x1F98/5
                if end_pos > payload_limit {
                    error!(
                        "[CN] TPDO mapping for 0x{:04X}/{} (offset {}, len {}) exceeds PRes payload limit {} bytes. Mapping invalid.",
                        entry.index, entry.sub_index, offset, length, payload_limit
                    );
                    // Return error according to spec 6.4.8.2 (E_PDO_MAP_OVERRUN)
                    // We don't have direct access to SDO abort here, signal via error
                    return Err(PowerlinkError::ValidationError("PDO mapping exceeds PRes payload limit"));
                }
                max_offset_len = max_offset_len.max(end_pos);

                // Read value from OD
                let Some(value_cow) = od.read(entry.index, entry.sub_index) else {
                    warn!(
                        "[CN] TPDO mapping for 0x{:04X}/{} failed: OD entry not found. Filling with zeros.",
                        entry.index, entry.sub_index
                    );
                    // Write zeros to the payload for this entry as per OD read failure
                    payload[offset..end_pos].fill(0);
                    continue; // Continue with next mapping entry
                };

                // Serialize and copy to payload buffer
                let serialized_data = value_cow.serialize();
                if serialized_data.len() != length {
                    warn!(
                        "[CN] TPDO mapping for 0x{:04X}/{} length mismatch. Mapped: {} bytes, Object: {} bytes. Truncating/Padding.",
                        entry.index, entry.sub_index, length, serialized_data.len()
                    );
                    let copy_len = serialized_data.len().min(length);
                    payload[offset..offset + copy_len].copy_from_slice(&serialized_data[..copy_len]);
                    // Zero out remaining bytes if object was shorter than mapping
                    if length > copy_len {
                        payload[offset + copy_len .. end_pos].fill(0);
                    }
                } else {
                    payload[offset..end_pos].copy_from_slice(&serialized_data);
                }
                trace!(
                    "[CN] Applied TPDO to PRes: Read {:?} from 0x{:04X}/{}",
                    value_cow, entry.index, entry.sub_index
                );
            }
        } else {
             trace!("[CN] TPDO Mapping object {:#06X} not found or sub-index 0 invalid.", mapping_index);
        }
    } else {
        trace!("[CN] TPDO Mapping object {:#06X} not found.", mapping_index);
    }


    // Truncate payload to the actual size needed based on mapping
    payload.truncate(max_offset_len);
    trace!("[CN] Built PRes payload with actual size: {}", payload.len());
    Ok((payload, pdo_version))
}