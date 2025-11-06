//! Utility functions for creating default Object Dictionaries.

use super::{
    data::{AccessType, Category, Object, ObjectValue, PdoMapping},
    entry::ObjectEntry,
    ObjectDictionary,
};
use crate::{
    nmt::flags::FeatureFlags,
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
        ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned32(0x000F0191)), // Generic I/O device
            name: "NMT_DeviceType_U32",
            category: Category::Mandatory,
            access: Some(AccessType::Constant),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );

    // 0x1006: NMT_CycleLen_U32
    od.insert(
        0x1006,
        ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned32(20000)), // 20ms
            name: "NMT_CycleLen_U32",
            category: Category::Mandatory,
            access: Some(AccessType::ReadWrite),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );

    // 0x1008: NMT_ManufactDevName_VS (Required for IdentResponse)
    od.insert(
        0x1008,
        ObjectEntry {
            object: Object::Variable(ObjectValue::VisibleString("powerlink-rs CN".into())),
            name: "NMT_ManufactDevName_VS",
            category: Category::Optional,
            access: Some(AccessType::Constant),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );

    // 0x1018: NMT_IdentityObject_REC
    od.insert(
        0x1018,
        ObjectEntry {
            object: Object::Record(vec![
                ObjectValue::Unsigned8(4), // Max sub-index
                ObjectValue::Unsigned32(0x12345678), // VendorId
                ObjectValue::Unsigned32(0x00000001), // ProductCode
                ObjectValue::Unsigned32(0x00010000), // RevisionNo
                ObjectValue::Unsigned32(0xABCDEF01), // SerialNo
            ]),
            name: "NMT_IdentityObject_REC",
            category: Category::Mandatory,
            access: Some(AccessType::Constant),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );

    // 0x1F82: NMT_FeatureFlags_U32 (Default: Isochronous + ASnd SDO)
    let cn_flags = FeatureFlags::ISOCHRONOUS | FeatureFlags::SDO_ASND;
    od.insert(
        0x1F82,
        ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned32(cn_flags.0)),
            name: "NMT_FeatureFlags_U32",
            category: Category::Mandatory,
            access: Some(AccessType::Constant),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );

    // 0x1F93: NMT_EPLNodeID_REC
    od.insert(
        0x1F93,
        ObjectEntry {
            object: Object::Record(vec![
                ObjectValue::Unsigned8(2), // Max sub-index
                ObjectValue::Unsigned8(node_id.0),
                ObjectValue::Boolean(0), // Node ID by HW = FALSE
            ]),
            name: "NMT_EPLNodeID_REC",
            category: Category::Mandatory,
            access: Some(AccessType::ReadWrite),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );

    // 0x1C14: DLL_CNLossOfSocTolerance_U32 (DS 301, 4.7.2.2)
    od.insert(
        0x1C14,
        ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned32(100000)), // 100ms
            name: "DLL_CNLossOfSocTolerance_U32",
            category: Category::Mandatory,
            access: Some(AccessType::ReadWrite),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
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
        ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned32(mn_flags.0)),
            name: "NMT_FeatureFlags_U32",
            category: Category::Mandatory,
            access: Some(AccessType::Constant),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );

    // --- MN Node Management Lists (DS 301, 7.2.2.4) ---
    // (Initialize as empty arrays, to be populated by the application)

    // 0x1F80: NMT_StartUp_U32
    od.insert(
        0x1F80,
        ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned32(0)),
            name: "NMT_StartUp_U32",
            category: Category::Mandatory,
            access: Some(AccessType::ReadWrite),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );

    // 0x1F84: NMT_MNNodeList_AU32 (Device Type List)
    od.insert(
        0x1F84,
        ObjectEntry::array(
            0x1F84,
            "NMT_MNNodeList_AU32",
            C_ADR_MN_DEF_NODE_ID, // Max sub-index
            Object::Variable(ObjectValue::Unsigned32(0)),
        ),
    );

    // 0x1F85: NMT_MNVendorIdList_AU32
    od.insert(
        0x1F85,
        ObjectEntry::array(
            0x1F85,
            "NMT_MNVendorIdList_AU32",
            C_ADR_MN_DEF_NODE_ID,
            Object::Variable(ObjectValue::Unsigned32(0)),
        ),
    );

    // 0x1F86: NMT_MNProductCodeList_AU32
    od.insert(
        0x1F86,
        ObjectEntry::array(
            0x1F86,
            "NMT_MNProductCodeList_AU32",
            C_ADR_MN_DEF_NODE_ID,
            Object::Variable(ObjectValue::Unsigned32(0)),
        ),
    );

    // 0x1F87: NMT_MNRevisionNoList_AU32
    od.insert(
        0x1F87,
        ObjectEntry::array(
            0x1F87,
            "NMT_MNRevisionNoList_AU32",
            C_ADR_MN_DEF_NODE_ID,
            Object::Variable(ObjectValue::Unsigned32(0)),
        ),
    );

    // 0x1F8D: DLL_MNPResPayloadLimitList_AU16
    od.insert(
        0x1F8D,
        ObjectEntry::array(
            0x1F8D,
            "DLL_MNPResPayloadLimitList_AU16",
            C_ADR_MN_DEF_NODE_ID,
            Object::Variable(ObjectValue::Unsigned16(0)),
        ),
    );

    // 0x1F92: DLL_MNPResTimeOut_AU32
    od.insert(
        0x1F92,
        ObjectEntry::array(
            0x1F92,
            "DLL_MNPResTimeOut_AU32",
            C_ADR_MN_DEF_NODE_ID,
            Object::Variable(ObjectValue::Unsigned32(100000)), // 100ms
        ),
    );

    od
}

/// Helper to add standard diagnostic objects (0x1101, 0x1102) to an OD.
fn add_diagnostic_objects(od: &mut ObjectDictionary<'static>) {
    // 0x1101: DIA_NMTTelegrCount_REC (DS 301, 8.1.1)
    od.insert(
        0x1101,
        ObjectEntry {
            object: Object::Record(vec![
                ObjectValue::Unsigned8(5), // Max sub-index
                ObjectValue::Unsigned32(0), // 1: IsochrCyc_U32
                ObjectValue::Unsigned32(0), // 2: IsochrRx_U32
                ObjectValue::Unsigned32(0), // 3: IsochrTx_U32
                ObjectValue::Unsigned32(0), // 4: AsyncRx_U32
                ObjectValue::Unsigned32(0), // 5: AsyncTx_U32
            ]),
            name: "DIA_NMTTelegrCount_REC",
            category: Category::Optional,
            access: Some(AccessType::ReadOnly),
            default_value: None,
            value_range: None,
            pdo_mapping: Some(PdoMapping::No),
        },
    );

    // 0x1102: DIA_ERRStatistics_REC (DS 301, 8.1.2)
    od.insert(
        0x1102,
        ObjectEntry {
            object: Object::Record(vec![
                ObjectValue::Unsigned8(2), // Max sub-index
                ObjectValue::Unsigned32(0), // 1: HistoryEntryWrite_U32
                ObjectValue::Unsigned32(0), // 2: EmergencyQueueOverflow_U32
            ]),
            name: "DIA_ERRStatistics_REC",
            category: Category::Optional,
            access: Some(AccessType::ReadOnly),
            default_value: None,
            value_range: None,
            pdo_mapping: Some(PdoMapping::No),
        },
    );
}