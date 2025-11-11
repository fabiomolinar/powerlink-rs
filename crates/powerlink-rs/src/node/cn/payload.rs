use crate::frame::basic::MacAddress;
use crate::frame::error::ErrorEntry;
use crate::frame::poll::{PResFlags, RSFlag};
use crate::frame::{ASndFrame, PResFrame, PowerlinkFrame, ServiceId};
use crate::nmt::NmtStateMachine;
use crate::nmt::events::NmtCommand;
use crate::nmt::states::NmtState;
use crate::od::{constants, ObjectValue}; // Import constants
use crate::pdo::PDOVersion;
use crate::sdo::SdoClient;
use crate::types::C_ADR_MN_DEF_NODE_ID;
use crate::{od::ObjectDictionary, types::NodeId, PowerlinkError};
use alloc::collections::VecDeque;
use alloc::vec;
use alloc::vec::Vec;
use log::{debug, error, trace, warn};

// Import the context directly for the new TPDO build method
use super::state::CnContext;

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
    context: &CnContext, // Use the full context
    sdo_client: &SdoClient,
    pending_nmt_requests: &[(NmtCommand, NodeId)],
    en_flag: bool,
) -> PowerlinkFrame {
    let node_id = context.nmt_state_machine.node_id();
    let nmt_state = context.nmt_state_machine.current_state();
    let mac_address = context.core.mac_address;

    debug!("Building PRes in response to PReq for node {}", node_id.0);

    // Call the new inherent method on CnContext
    let (payload, pdo_version, payload_is_valid) = match context.build_tpdo_payload() {
        Ok((payload, version)) => (payload, version, true),
        Err(e) => {
            error!(
                "Failed to build TPDO payload for PRes: {:?}. Sending empty PRes with RD=0.",
                e
            );
            // Must still send a PRes of the correct configured size, even if empty
            let payload_limit = context
                .core
                .od
                .read_u16(
                    constants::IDX_NMT_CYCLE_TIMING_REC,
                    constants::SUBIDX_NMT_CYCLE_TIMING_PRES_ACT_PAYLOAD_U16,
                )
                .unwrap_or(36) as usize;
            (vec![0u8; payload_limit], PDOVersion(0), false)
        }
    };

    // The RD flag is set only if the NMT state is Operational AND the PDO payload was built successfully.
    let rd_flag = (nmt_state == NmtState::NmtOperational) && payload_is_valid;

    // Check for pending SDO/NMT requests to set RS and PR flags.
    // NMT requests (PR=7) have higher priority than generic SDO requests (PR=3).
    let (rs_count, pr_flag) = if !pending_nmt_requests.is_empty() {
        (
            pending_nmt_requests.len().min(7) as u8,
            crate::frame::PRFlag::PrioNmtRequest,
        )
    } else {
        sdo_client.pending_request_count_and_priority()
    };

    let flags = PResFlags {
        rd: rd_flag,
        en: en_flag,
        rs: RSFlag::new(rs_count),
        pr: pr_flag,
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
    payload[2] = od
        .read_u8(constants::IDX_NMT_CURR_NMT_STATE_U8, 0)
        .unwrap_or(0);
    // Reserved (Octet 3)
    // EPLVersion (Octet 4) - from 0x1F83/0
    payload[4] = od
        .read_u8(constants::IDX_NMT_EPL_VERSION_U8, 0)
        .unwrap_or(0);
    // Reserved (Octet 5)
    // FeatureFlags (Octets 6-9) - from 0x1F82/0
    payload[6..10].copy_from_slice(
        &od.read_u32(constants::IDX_NMT_FEATURE_FLAGS_U32, 0)
            .unwrap_or(0)
            .to_le_bytes(),
    );
    // MTU (Octets 10-11) - from 0x1F98/8 (AsyncMTU_U16)
    payload[10..12].copy_from_slice(
        &od.read_u16(
            constants::IDX_NMT_CYCLE_TIMING_REC,
            constants::SUBIDX_NMT_CYCLE_TIMING_ASYNC_MTU_U16,
        )
        .unwrap_or(0)
        .to_le_bytes(),
    );
    // PollInSize (Octets 12-13) - from 0x1F98/4 (PreqActPayloadLimit_U16)
    payload[12..14].copy_from_slice(
        &od.read_u16(
            constants::IDX_NMT_CYCLE_TIMING_REC,
            constants::SUBIDX_NMT_CYCLE_TIMING_PREQ_ACT_PAYLOAD_U16,
        )
        .unwrap_or(0)
        .to_le_bytes(),
    );
    // PollOutSize (Octets 14-15) - from 0x1F98/5 (PresActPayloadLimit_U16)
    payload[14..16].copy_from_slice(
        &od.read_u16(
            constants::IDX_NMT_CYCLE_TIMING_REC,
            constants::SUBIDX_NMT_CYCLE_TIMING_PRES_ACT_PAYLOAD_U16,
        )
        .unwrap_or(0)
        .to_le_bytes(),
    );
    // ResponseTime (Octets 16-19) - from 0x1F98/3 (PresMaxLatency_U32)
    payload[16..20].copy_from_slice(
        &od.read_u32(
            constants::IDX_NMT_CYCLE_TIMING_REC,
            constants::SUBIDX_NMT_CYCLE_TIMING_PRES_MAX_LATENCY_U32,
        )
        .unwrap_or(0)
        .to_le_bytes(),
    );
    // Reserved (Octets 20-21)
    // DeviceType (Octets 22-25) - from 0x1000/0
    payload[22..26].copy_from_slice(
        &od.read_u32(constants::IDX_NMT_DEVICE_TYPE_U32, 0)
            .unwrap_or(0)
            .to_le_bytes(),
    );
    // VendorID (Octets 26-29) - from 0x1018/1
    payload[26..30].copy_from_slice(
        &od.read_u32(constants::IDX_NMT_IDENTITY_OBJECT_REC, 1)
            .unwrap_or(0)
            .to_le_bytes(),
    );
    // ProductCode (Octets 30-33) - from 0x1018/2
    payload[30..34].copy_from_slice(
        &od.read_u32(constants::IDX_NMT_IDENTITY_OBJECT_REC, 2)
            .unwrap_or(0)
            .to_le_bytes(),
    );
    // RevisionNumber (Octets 34-37) - from 0x1018/3
    payload[34..38].copy_from_slice(
        &od.read_u32(constants::IDX_NMT_IDENTITY_OBJECT_REC, 3)
            .unwrap_or(0)
            .to_le_bytes(),
    );
    // SerialNumber (Octets 38-41) - from 0x1018/4
    payload[38..42].copy_from_slice(
        &od.read_u32(constants::IDX_NMT_IDENTITY_OBJECT_REC, 4)
            .unwrap_or(0)
            .to_le_bytes(),
    );
    // VendorSpecificExtension1 (Octets 42-49) - Skipped (zeros)
    // VerifyConfigurationDate (Octets 50-53) - from 0x1020/1
    payload[50..54].copy_from_slice(
        &od.read_u32(constants::IDX_CFM_VERIFY_CONFIG_REC, 1)
            .unwrap_or(0)
            .to_le_bytes(),
    );
    // VerifyConfigurationTime (Octets 54-57) - from 0x1020/2
    payload[54..58].copy_from_slice(
        &od.read_u32(constants::IDX_CFM_VERIFY_CONFIG_REC, 2)
            .unwrap_or(0)
            .to_le_bytes(),
    );
    // ApplicationSwDate (Octets 58-61) - from 0x1F52/1
    payload[58..62].copy_from_slice(
        &od.read_u32(constants::IDX_PDL_LOC_VER_APPL_SW_REC, 1)
            .unwrap_or(0)
            .to_le_bytes(),
    );
    // ApplicationSwTime (Octets 62-65) - from 0x1F52/2
    payload[62..66].copy_from_slice(
        &od.read_u32(constants::IDX_PDL_LOC_VER_APPL_SW_REC, 2)
            .unwrap_or(0)
            .to_le_bytes(),
    );
    // IPAddress (Octets 66-69) - from 0x1E40/2
    payload[66..70].copy_from_slice(
        &od.read_u32(constants::IDX_NWL_IP_ADDR_TABLE_REC, 2)
            .unwrap_or(0)
            .to_le_bytes(),
    );
    // SubnetMask (Octets 70-73) - from 0x1E40/3
    payload[70..74].copy_from_slice(
        &od.read_u32(constants::IDX_NWL_IP_ADDR_TABLE_REC, 3)
            .unwrap_or(0)
            .to_le_bytes(),
    );
    // DefaultGateway (Octets 74-77) - from 0x1E40/5
    payload[74..78].copy_from_slice(
        &od.read_u32(constants::IDX_NWL_IP_ADDR_TABLE_REC, 5)
            .unwrap_or(0)
            .to_le_bytes(),
    );
    // HostName (Octets 78-109) - from 0x1F9A/0 (VISIBLE_STRING32)
    if let Some(cow_val) = od.read(constants::IDX_NMT_HOST_NAME_VSTR, 0) {
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
    // The payload limit is defined by AsyncMTU (0x1F98/8), minus ASnd header (4 bytes)
    let mtu = od
        .read_u16(
            constants::IDX_NMT_CYCLE_TIMING_REC,
            constants::SUBIDX_NMT_CYCLE_TIMING_ASYNC_MTU_U16,
        )
        .unwrap_or(300) as usize;
    let max_payload_size = mtu.saturating_sub(4); // Corrected typo

    // Base payload with fixed fields
    let mut payload = vec![0u8; 14];
    // Octet 0: Flags (EN, EC)
    if en_flag {
        payload[0] |= 1 << 5;
    }
    if ec_flag {
        payload[0] |= 1 << 4;
    }
    // Octet 1: Flags (PR, RS)
    // Octet 2: NMTState
    payload[2] = od
        .read_u8(constants::IDX_NMT_CURR_NMT_STATE_U8, 0)
        .unwrap_or(0);
    // Octets 3-5: Reserved
    // Octets 6-13: StaticErrorBitField
    payload[6] = od
        .read_u8(constants::IDX_NMT_ERROR_REGISTER_U8, 0)
        .unwrap_or(0);
    // Bytes 7-13 are reserved or device specific errors.

    // Append Error/Event History Entries from emergency queue
    let mut entry_buffer = [0u8; 20]; // Buffer to serialize one entry
    while let Some(entry) = emergency_queue.front() {
        if payload.len() + 20 > max_payload_size {
            warn!(
                "[CN] Not enough space in StatusResponse for all queued errors. Remaining: {}",
                emergency_queue.len()
            );
            break;
        }

        entry_buffer.fill(0);
        entry_buffer[0..2].copy_from_slice(
            &((entry.entry_type.profile & 0x0FFF)
                | ((entry.entry_type.mode as u16) << 12)
                | if entry.entry_type.is_status_entry {
                    1 << 15
                } else {
                    0
                }
                | if entry.entry_type.send_to_queue {
                    1 << 14
                } else {
                    0
                })
            .to_le_bytes(),
        );
        entry_buffer[2..4].copy_from_slice(&entry.error_code.to_le_bytes());
        entry_buffer[4..8].copy_from_slice(&entry.timestamp.seconds.to_le_bytes());
        entry_buffer[8..12].copy_from_slice(&entry.timestamp.nanoseconds.to_le_bytes());
        entry_buffer[12..20].copy_from_slice(&entry.additional_information.to_le_bytes());

        payload.extend_from_slice(&entry_buffer);
        emergency_queue.pop_front();
        trace!(
            "[CN] Added error entry to StatusResponse. Queue size: {}",
            emergency_queue.len()
        );
    }

    if payload.len() + 20 <= max_payload_size {
        payload.extend_from_slice(&[0u8; 20]); // Add terminator entry
    }

    payload
}