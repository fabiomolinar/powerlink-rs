// crates/powerlink-rs/src/node/cn/payload.rs
use crate::frame::basic::MacAddress;
use crate::frame::control::{IdentResponsePayload, StaticErrorBitField, StatusResponsePayload};
use crate::frame::error::ErrorEntry;
use crate::frame::poll::{PResFlags, RSFlag};
use crate::frame::{ASndFrame, PResFrame, PowerlinkFrame, ServiceId};
use crate::nmt::NmtStateMachine;
use crate::nmt::events::CnNmtRequest;
use crate::nmt::states::NmtState;
use crate::od::constants;
use crate::pdo::PDOVersion;
use crate::sdo::SdoClient;
use crate::types::C_ADR_MN_DEF_NODE_ID;
use crate::{od::ObjectDictionary, types::NodeId};
use alloc::collections::VecDeque;
use alloc::vec;
use alloc::vec::Vec;
use log::{debug, error};

use super::state::CnContext;

pub(super) fn build_ident_response(
    mac_address: MacAddress,
    node_id: NodeId,
    od: &ObjectDictionary,
    soa: &crate::frame::SoAFrame,
    sdo_client: &SdoClient,
    pending_nmt_requests: &[(CnNmtRequest, NodeId)],
) -> PowerlinkFrame {
    debug!("Building IdentResponse for SoA from node {}", soa.source.0);

    let mut payload_struct = IdentResponsePayload::new(od);

    let (rs_count, pr_flag) = if !pending_nmt_requests.is_empty() {
        (
            pending_nmt_requests.len().min(7) as u8,
            crate::frame::PRFlag::PrioNmtRequest,
        )
    } else {
        sdo_client.pending_request_count_and_priority()
    };
    payload_struct.pr = pr_flag;
    payload_struct.rs = RSFlag::new(rs_count);

    // FIX: Use a buffer large enough for the struct (approx 180 bytes max).
    // Standard says IdentResponse is fixed length logic mostly.
    // We init with 0 to pad string fields correctly.
    let mut payload_buf = vec![0u8; 256]; 
    let payload_len = match payload_struct.serialize(&mut payload_buf) {
        Ok(len) => len,
        Err(e) => {
            error!("Failed to serialize IdentResponsePayload: {:?}", e);
            // Fallback: minimal length or zeroed
            158 
        }
    };
    // Truncate to the actual written length so the frame is sized correctly.
    payload_buf.truncate(payload_len);

    let asnd = ASndFrame::new(
        mac_address,
        soa.eth_header.source_mac,    
        NodeId(C_ADR_MN_DEF_NODE_ID), 
        node_id,
        ServiceId::IdentResponse,
        payload_buf, 
    );
    PowerlinkFrame::ASnd(asnd)
}

pub(super) fn build_status_response(
    mac_address: MacAddress,
    node_id: NodeId,
    od: &mut ObjectDictionary,
    en_flag: bool,
    ec_flag: bool,
    emergency_queue: &mut VecDeque<ErrorEntry>,
    soa: &crate::frame::SoAFrame,
    sdo_client: &SdoClient,
    pending_nmt_requests: &[(CnNmtRequest, NodeId)],
) -> PowerlinkFrame {
    debug!("Building StatusResponse for SoA from node {}", soa.source.0);

    let nmt_state = od
        .read_u8(constants::IDX_NMT_CURR_NMT_STATE_U8, 0)
        .and_then(|val| NmtState::try_from(val).ok())
        .unwrap_or(NmtState::NmtNotActive);

    let static_errors = StaticErrorBitField::new(od);

    let (rs_count, pr_flag) = if !pending_nmt_requests.is_empty() {
        (
            pending_nmt_requests.len().min(7) as u8,
            crate::frame::PRFlag::PrioNmtRequest,
        )
    } else {
        sdo_client.pending_request_count_and_priority()
    };

    let mtu = od
        .read_u16(
            constants::IDX_NMT_CYCLE_TIMING_REC,
            constants::SUBIDX_NMT_CYCLE_TIMING_ASYNC_MTU_U16,
        )
        .unwrap_or(300) as usize;
    let max_payload_size = mtu.saturating_sub(4);

    let max_entries = (max_payload_size.saturating_sub(14 + 20)) / 20;

    let entries: Vec<ErrorEntry> = emergency_queue
        .drain(..max_entries.min(emergency_queue.len()))
        .collect();

    if !entries.is_empty() {
        od.increment_counter(
            constants::IDX_DIAG_ERR_STATISTICS_REC,
            constants::SUBIDX_DIAG_ERR_STATS_EMCY_WRITE,
        );
    }

    let mut payload_struct = StatusResponsePayload::new(
        en_flag,
        ec_flag,
        pr_flag,
        RSFlag::new(rs_count),
        nmt_state,
        static_errors,
        entries,
    );

    let mut payload_buf = vec![0u8; max_payload_size];
    let payload_len = match payload_struct.serialize(&mut payload_buf) {
        Ok(len) => len,
        Err(e) => {
            error!("Failed to serialize StatusResponsePayload: {:?}", e);
            payload_buf.truncate(14 + 20);
            payload_buf.fill(0);
            payload_struct.error_entries = Vec::new();
            payload_struct
                .serialize(&mut payload_buf)
                .unwrap_or(14 + 20)
        }
    };
    payload_buf.truncate(payload_len);

    let asnd = ASndFrame::new(
        mac_address,
        soa.eth_header.source_mac,
        NodeId(C_ADR_MN_DEF_NODE_ID),
        node_id,
        ServiceId::StatusResponse,
        payload_buf,
    );
    PowerlinkFrame::ASnd(asnd)
}

pub(super) fn build_nmt_request(
    mac_address: MacAddress,
    node_id: NodeId,
    command_id: u8,
    target: NodeId,
    soa: &crate::frame::SoAFrame,
) -> PowerlinkFrame {
    debug!(
        "Building NMTRequest(CommandID={:#04x}, Target={}) for SoA from node {}",
        command_id, target.0, soa.source.0
    );
    let payload = vec![
        command_id,
        target.0,
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

pub(super) fn build_pres_response(context: &mut CnContext, en_flag: bool) -> PowerlinkFrame {
    let node_id = context.nmt_state_machine.node_id();
    let nmt_state = context.nmt_state_machine.current_state();
    let mac_address = context.core.mac_address;

    debug!("Building PRes in response to PReq for node {}", node_id.0);

    let (payload, pdo_version, payload_is_valid) = match context.build_tpdo_payload() {
        Ok((payload, version)) => (payload, version, true),
        Err(e) => {
            error!(
                "Failed to build TPDO payload for PRes: {:?}. Sending empty PRes with RD=0.",
                e
            );
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

    let rd_flag = (nmt_state == NmtState::NmtOperational) && payload_is_valid;

    let (rs_count, pr_flag) = if !context.pending_nmt_requests.is_empty() {
        (
            context.pending_nmt_requests.len().min(7) as u8,
            crate::frame::PRFlag::PrioNmtRequest,
        )
    } else {
        context.core.sdo_client.pending_request_count_and_priority()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::control::{IdentResponsePayload, StatusResponsePayload, SoAFlags};
    use crate::frame::{SoAFrame, ServiceId, RequestedServiceId};
    use crate::nmt::states::NmtState;
    use crate::od::{ObjectDictionary, ObjectEntry, ObjectValue, Object};
    use crate::types::{EPLVersion, NodeId};
    use crate::sdo::SdoClient;
    use crate::frame::basic::MacAddress;
    use alloc::collections::VecDeque;
    use alloc::vec;

    fn setup_od() -> ObjectDictionary<'static> {
        let mut od = ObjectDictionary::new(None);
        
        // 0x1018 Identity
        od.insert(0x1018, ObjectEntry { 
            object: Object::Record(vec![
                ObjectValue::Unsigned32(0), 
                ObjectValue::Unsigned32(0), 
                ObjectValue::Unsigned32(0), 
                ObjectValue::Unsigned32(0)
            ]), 
            ..Default::default() 
        });
        od.write(0x1018, 1, ObjectValue::Unsigned32(0x11112222)).unwrap();
        od.write(0x1018, 2, ObjectValue::Unsigned32(0x33334444)).unwrap();
        
        od.insert(0x1000, ObjectEntry { object: Object::Variable(ObjectValue::Unsigned32(0x00009999)), ..Default::default() });
        od.insert(0x1F9A, ObjectEntry { object: Object::Variable(ObjectValue::VisibleString("TestCN".into())), ..Default::default() });
        
        od.insert(constants::IDX_NMT_CURR_NMT_STATE_U8, ObjectEntry { object: Object::Variable(ObjectValue::Unsigned8(0)), ..Default::default() });
        od.insert(constants::IDX_NMT_ERROR_REGISTER_U8, ObjectEntry { object: Object::Variable(ObjectValue::Unsigned8(0)), ..Default::default() });

        // 0x1E40 Network Configuration (Explicitly required by IdentResponsePayload::new)
        od.insert(0x1E40, ObjectEntry {
            object: Object::Record(vec![
                ObjectValue::Unsigned32(0), // Sub 1: IP
                ObjectValue::Unsigned32(0), // Sub 2: Mask
                ObjectValue::Unsigned32(0)  // Sub 3: Gateway
            ]),
            ..Default::default()
        });

        // 0x1020 Verify Configuration (Some serialization logic checks this)
        od.insert(0x1020, ObjectEntry {
            object: Object::Record(vec![
                ObjectValue::Unsigned32(0), // Date
                ObjectValue::Unsigned32(0)  // Time
            ]),
            ..Default::default()
        });

        od
    }

    #[test]
    fn test_build_ident_response() {
        let od = setup_od();
        let sdo_client = SdoClient::new();
        let pending_nmt = Vec::new();
        
        let soa = SoAFrame::new(
            MacAddress::default(),
            NmtState::NmtOperational,
            SoAFlags::default(),
            RequestedServiceId::NoService,
            NodeId(0),
            EPLVersion(0x20)
        );
        let soa_frame = PowerlinkFrame::SoA(soa);
        let soa_ref = match &soa_frame { PowerlinkFrame::SoA(s) => s, _ => panic!() };

        let frame = build_ident_response(
            MacAddress::default(),
            NodeId(10),
            &od,
            soa_ref,
            &sdo_client,
            &pending_nmt
        );

        if let PowerlinkFrame::ASnd(asnd) = frame {
            assert_eq!(asnd.service_id, ServiceId::IdentResponse);
            
            // Match and print specific error if deserialization fails
            match IdentResponsePayload::deserialize(&asnd.payload) {
                Ok(payload) => {
                    assert_eq!(payload.vendor_id, 0x11112222);
                    assert_eq!(payload.product_code, 0x33334444);
                    assert_eq!(payload.device_type, 0x00009999);
                    assert_eq!(payload.host_name.as_str(), "TestCN");
                },
                Err(e) => {
                    panic!("Failed to deserialize IdentResponse. Payload size: {}: {:?}", asnd.payload.len(), e);
                }
            }
        } else {
            panic!("Wrong frame type returned");
        }
    }

    #[test]
    fn test_build_status_response_flags() {
        let mut od = setup_od();
        let sdo_client = SdoClient::new();
        let mut emergency_queue = VecDeque::new();
        let pending_nmt = Vec::new();

        let soa = SoAFrame::new(
            MacAddress::default(),
            NmtState::NmtOperational,
            SoAFlags::default(),
            RequestedServiceId::NoService,
            NodeId(0),
            EPLVersion(0x20)
        );
        let soa_frame = PowerlinkFrame::SoA(soa);
        let soa_ref = match &soa_frame { PowerlinkFrame::SoA(s) => s, _ => panic!() };

        let frame = build_status_response(
            MacAddress::default(),
            NodeId(10),
            &mut od,
            true,  
            false, 
            &mut emergency_queue,
            soa_ref,
            &sdo_client,
            &pending_nmt
        );

        if let PowerlinkFrame::ASnd(asnd) = frame {
            assert_eq!(asnd.service_id, ServiceId::StatusResponse);
            // Ensure payload is valid
             match StatusResponsePayload::deserialize(&asnd.payload) {
                Ok(payload) => assert!(payload.en_flag),
                Err(e) => panic!("Failed to deserialize StatusResponse: {:?}", e),
            }
        } else {
            panic!("Wrong frame type");
        }
    }
}