// crates/powerlink-rs/src/od/utils.rs

//! Utility functions for creating default Object Dictionaries.

use super::{Object, ObjectDictionary, ObjectEntry, ObjectValue};
use crate::{
    nmt::flags::FeatureFlags,
    od::{AccessType, Category, PdoMapping},
    types::{C_ADR_MN_DEF_NODE_ID, NodeId},
};
use alloc::vec;

/// Creates a minimal, compliant Object Dictionary for a POWERLINK
/// Controlled Node (CN).
///
/// This populates the OD with all mandatory objects required for a
/// CN to be identified and brought to an operational state.
pub fn new_cn_default(node_id: NodeId) -> ObjectDictionary<'static> {
    let mut od = ObjectDictionary::new(None);

    // --- Mandatory Objects (DS 301, 7.2.2.1.1) ---

    // 0x1000: NMT_DeviceType_U32
    od.insert(
        0x1000,
        ObjectEntry::variable(
            0x1000,
            "NMT_DeviceType_U32",
            ObjectValue::Unsigned32(0x000F0191), // Generic I/O device
        )
        .with_access(AccessType::Constant),
    );

    // 0x1006: NMT_CycleLen_U32
    od.insert(
        0x1006,
        ObjectEntry::variable(0x1006, "NMT_CycleLen_U32", ObjectValue::Unsigned32(20000)) // 20ms
            .with_access(AccessType::ReadWrite),
    );

    // 0x1008: NMT_ManufactDevName_VS (Required for IdentResponse)
    od.insert(
        0x1008,
        ObjectEntry::variable(
            0x1008,
            "NMT_ManufactDevName_VS",
            ObjectValue::VisibleString("powerlink-rs CN".into()),
        )
        .with_category(Category::Optional)
        .with_access(AccessType::Constant),
    );

    // 0x1018: NMT_IdentityObject_REC
    od.insert(
        0x1018,
        ObjectEntry::record(
            0x1018,
            "NMT_IdentityObject_REC",
            vec![
                ObjectValue::Unsigned32(0x12345678), // VendorId
                ObjectValue::Unsigned32(0x00000001), // ProductCode
                ObjectValue::Unsigned32(0x00010000), // RevisionNo
                ObjectValue::Unsigned32(0xABCDEF01), // SerialNo
            ],
        )
        .with_access(AccessType::Constant),
    );

    // 0x1F82: NMT_FeatureFlags_U32 (Default: Isochronous + ASnd SDO)
    let cn_flags = FeatureFlags::ISOCHRONOUS | FeatureFlags::SDO_ASND;
    od.insert(
        0x1F82,
        ObjectEntry::variable(
            0x1F82,
            "NMT_FeatureFlags_U32",
            ObjectValue::Unsigned32(cn_flags.0),
        )
        .with_access(AccessType::Constant),
    );

    // 0x1F93: NMT_EPLNodeID_REC
    od.insert(
        0x1F93,
        ObjectEntry::record(
            0x1F93,
            "NMT_EPLNodeID_REC",
            vec![
                ObjectValue::Unsigned8(node_id.0),
                ObjectValue::Boolean(0), // Node ID by HW = FALSE
            ],
        )
        .with_access(AccessType::ReadWrite),
    );

    // 0x1C14: DLL_CNLossOfSocTolerance_U32 (DS 301, 4.7.2.2)
    od.insert(
        0x1C14,
        ObjectEntry::variable(
            0x1C14,
            "DLL_CNLossOfSocTolerance_U32",
            ObjectValue::Unsigned32(100000), // 100ms
        )
        .with_access(AccessType::ReadWrite),
    );

    // --- Diagnostic Counters (DS 301, 8.1) ---
    // (Included for monitor compatibility)
    add_diagnostic_objects(&mut od);

    od
}

/// Creates a minimal, compliant Object Dictionary for a POWERLINK
/// Managing Node (MN).
///
/// This populates the OD with all mandatory objects required for an MN,
/// including the node management lists (0x1F8x) and diagnostic counters.
pub fn new_mn_default(node_id: NodeId) -> ObjectDictionary<'static> {
    // Start with a CN default OD
    let mut od = new_cn_default(node_id);

    // --- Modify/Add MN-Specific Objects ---

    // 0x1F82: NMT_FeatureFlags_U32 (Add MN flag)
    let mn_flags = FeatureFlags::ISOCHRONOUS | FeatureFlags::SDO_ASND | FeatureFlags::MANAGING_NODE;
    od.insert(
        0x1F82,
        ObjectEntry::variable(
            0x1F82,
            "NMT_FeatureFlags_U32",
            ObjectValue::Unsigned32(mn_flags.0),
        )
        .with_access(AccessType::Constant),
    );

    // --- MN Node Management Lists (DS 301, 7.2.2.4) ---
    // (Initialize as empty arrays, to be populated by the application)

    // 0x1F80: NMT_StartUp_U32
    od.insert(
        0x1F80,
        ObjectEntry::variable(0x1F80, "NMT_StartUp_U32", ObjectValue::Unsigned32(0))
            .with_access(AccessType::ReadWrite),
    );

    // 0x1F84: NMT_MNNodeList_AU32 (Device Type List)
    od.insert(
        0x1F84,
        ObjectEntry::array(
            0x1F84,
            "NMT_MNNodeList_AU32",
            C_ADR_MN_DEF_NODE_ID,
            Object::Variable(ObjectValue::Unsigned32(0)),
        )
        .with_access(AccessType::ReadWrite),
    );

    // 0x1F85: NMT_MNVendorIdList_AU32
    od.insert(
        0x1F85,
        ObjectEntry::array(
            0x1F85,
            "NMT_MNVendorIdList_AU32",
            C_ADR_MN_DEF_NODE_ID,
            Object::Variable(ObjectValue::Unsigned32(0)),
        )
        .with_access(AccessType::ReadWrite),
    );

    // 0x1F86: NMT_MNProductCodeList_AU32
    od.insert(
        0x1F86,
        ObjectEntry::array(
            0x1F86,
            "NMT_MNProductCodeList_AU32",
            C_ADR_MN_DEF_NODE_ID,
            Object::Variable(ObjectValue::Unsigned32(0)),
        )
        .with_access(AccessType::ReadWrite),
    );

    // 0x1F87: NMT_MNRevisionNoList_AU32
    od.insert(
        0x1F87,
        ObjectEntry::array(
            0x1F87,
            "NMT_MNRevisionNoList_AU32",
            C_ADR_MN_DEF_NODE_ID,
            Object::Variable(ObjectValue::Unsigned32(0)),
        )
        .with_access(AccessType::ReadWrite),
    );

    // 0x1F8D: DLL_MNPResPayloadLimitList_AU16
    od.insert(
        0x1F8D,
        ObjectEntry::array(
            0x1F8D,
            "DLL_MNPResPayloadLimitList_AU16",
            C_ADR_MN_DEF_NODE_ID,
            Object::Variable(ObjectValue::Unsigned16(0)),
        )
        .with_access(AccessType::ReadWrite),
    );

    // 0x1F92: DLL_MNPResTimeOut_AU32
    od.insert(
        0x1F92,
        ObjectEntry::array(
            0x1F92,
            "DLL_MNPResTimeOut_AU32",
            C_ADR_MN_DEF_NODE_ID,
            Object::Variable(ObjectValue::Unsigned32(100000)), // 100ms
        )
        .with_access(AccessType::ReadWrite),
    );

    od
}

/// Helper to add standard diagnostic objects (0x1101, 0x1102) to an OD.
fn add_diagnostic_objects(od: &mut ObjectDictionary<'static>) {
    // 0x1101: DIA_NMTTelegrCount_REC (DS 301, 8.1.1)
    od.insert(
        0x1101,
        ObjectEntry::record(
            0x1101,
            "DIA_NMTTelegrCount_REC",
            vec![
                ObjectValue::Unsigned32(0), // 1: IsochrCyc_U32
                ObjectValue::Unsigned32(0), // 2: IsochrRx_U32
                ObjectValue::Unsigned32(0), // 3: IsochrTx_U32
                ObjectValue::Unsigned32(0), // 4: AsyncRx_U32
                ObjectValue::Unsigned32(0), // 5: AsyncTx_U32
            ],
        )
        .with_category(Category::Optional)
        .with_access(AccessType::ReadOnly)
        .with_pdo_mapping(PdoMapping::No),
    );

    // 0x1102: DIA_ERRStatistics_REC (DS 301, 8.1.2)
    od.insert(
        0x1102,
        ObjectEntry::record(
            0x1102,
            "DIA_ERRStatistics_REC",
            vec![
                ObjectValue::Unsigned32(0), // 1: HistoryEntryWrite_U32
                ObjectValue::Unsigned32(0), // 2: EmergencyQueueOverflow_U32
            ],
        )
        .with_category(Category::Optional)
        .with_access(AccessType::ReadOnly)
        .with_pdo_mapping(PdoMapping::No),
    );
}