// crates/powerlink-rs/src/od/utils.rs
//! Utility functions for creating default Object Dictionaries.

use super::{
    entry::ObjectEntry,
    ObjectDictionary,
    {AccessType, Category, Object, ObjectValue, PdoMapping},
};
use crate::{nmt::flags::FeatureFlags, types::NodeId, PowerlinkError};
use alloc::vec;

/// Creates a minimal, compliant Object Dictionary for a POWERLINK
/// Controlled Node (CN).
///
/// This populates the OD with all mandatory objects required for a
/// CN to be identified and brought to an operational state.
///
/// Returns `Result` to allow for future fallible validation or allocation checks.
pub fn new_cn_default(node_id: NodeId) -> Result<ObjectDictionary<'static>, PowerlinkError> {
    let mut od = ObjectDictionary::new(None);

    // --- Mandatory Objects (DS 301, 7.2.2.1.1) ---

    // 0x1000: NMT_DeviceType_U32
    od.insert(
        0x1000,
        ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned32(0x000F0191)), // Generic I/O device
            name: "NMT_DeviceType_U32",
            category: Category::Mandatory,
            access: Some(AccessType::Constant), // Device Type is usually constant
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );

    // 0x1001: NMT_ErrorRegister_U8
    od.insert(
        0x1001,
        ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned8(0)),
            name: "NMT_ErrorRegister_U8",
            category: Category::Mandatory,
            access: Some(AccessType::ReadOnly), // CN writes internally, MN reads via SDO
            default_value: Some(ObjectValue::Unsigned8(0)),
            value_range: None,
            pdo_mapping: Some(PdoMapping::Optional), // Can be mapped to PDO
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

    // 0x1008: NMT_ManufactDevName_VS
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

    // 0x1010: NMT_StoreParam_REC (Essential for CFM)
    od.insert(
        0x1010,
        ObjectEntry {
            object: Object::Record(vec![
                // Sub-index 0 (Count) is implicit. Vector starts at Sub 1.
                ObjectValue::Unsigned32(0), // 1: All parameters
                ObjectValue::Unsigned32(0), // 2: Communication
                ObjectValue::Unsigned32(0), // 3: Application
                ObjectValue::Unsigned32(0), // 4: Manufacturer
            ]),
            name: "NMT_StoreParam_REC",
            category: Category::Optional,
            access: Some(AccessType::ReadWrite),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );

    // 0x1011: NMT_RestoreParam_REC
    od.insert(
        0x1011,
        ObjectEntry {
            object: Object::Record(vec![
                // Sub-index 0 (Count) is implicit.
                ObjectValue::Unsigned32(0), // 1: All parameters
                ObjectValue::Unsigned32(0), // 2: Communication
                ObjectValue::Unsigned32(0), // 3: Application
                ObjectValue::Unsigned32(0), // 4: Manufacturer
            ]),
            name: "NMT_RestoreParam_REC",
            category: Category::Optional,
            access: Some(AccessType::ReadWrite),
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
                // Sub-index 0 (Count) is implicit.
                ObjectValue::Unsigned32(0x12345678), // 1: VendorId
                ObjectValue::Unsigned32(0x00000001), // 2: ProductCode
                ObjectValue::Unsigned32(0x00010000), // 3: RevisionNo
                ObjectValue::Unsigned32(0xABCDEF01), // 4: SerialNo
            ]),
            name: "NMT_IdentityObject_REC",
            category: Category::Mandatory,
            access: Some(AccessType::Constant),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );

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
                // Sub-index 0 (Count) is implicit.
                ObjectValue::Unsigned8(node_id.0), // 1: NodeID
                ObjectValue::Boolean(0),           // 2: NodeIDByHW
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
            object: Object::Variable(ObjectValue::Unsigned32(100000)),
            name: "DLL_CNLossOfSocTolerance_U32",
            category: Category::Mandatory,
            access: Some(AccessType::ReadWrite),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );

    // 0x1F99: NMT_CNBasicEthernetTimeout_U32
    od.insert(
        0x1F99,
        ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned32(5_000_000)), // 5 seconds
            name: "NMT_CNBasicEthernetTimeout_U32",
            category: Category::Mandatory,
            access: Some(AccessType::ReadWrite),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );

    add_diagnostic_objects(&mut od)?;

    Ok(od)
}

/// Creates a minimal, compliant Object Dictionary for a POWERLINK
/// Managing Node (MN).
pub fn new_mn_default(node_id: NodeId) -> Result<ObjectDictionary<'static>, PowerlinkError> {
    // Start with a CN default OD
    let mut od = new_cn_default(node_id)?;

    // --- Modify/Add MN-Specific Objects ---

    // 0x1F82: NMT_FeatureFlags_U32 (Add MN flag)
    // Note: This overwrites the entry created by new_cn_default
    let mn_flags = FeatureFlags::ISOCHRONOUS | FeatureFlags::SDO_ASND;
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

    // 0x1F81: NMT_NodeAssignment_AU32
    // 254 entries (Node 1..254). Sub 0 is implicit.
    od.insert(
        0x1F81,
        ObjectEntry {
            object: Object::Array(vec![ObjectValue::Unsigned32(0); 254]),
            name: "NMT_NodeAssignment_AU32",
            category: Category::Mandatory,
            access: Some(AccessType::ReadWrite),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );

    // 0x1F89: NMT_BootTime_REC
    od.insert(
        0x1F89,
        ObjectEntry {
            object: Object::Record(vec![
                // Sub-index 0 (Count) is implicit.
                ObjectValue::Unsigned32(1_000_000), // 1: MNWaitNotAct_U32
                ObjectValue::Unsigned32(500_000),   // 2: MNTimeoutPreOp1_U32
                ObjectValue::Unsigned32(500_000),   // 3: MNWaitPreOp1_U32
                ObjectValue::Unsigned32(500_000),   // 4: MNTimeoutPreOp2_U32
                ObjectValue::Unsigned32(500_000),   // 5: MNTimeoutReadyToOp_U32
                ObjectValue::Unsigned32(500_000),   // 6: MNIdentificationTimeout_U32
                ObjectValue::Unsigned32(500_000),   // 7: MNSoftwareTimeout_U32
                ObjectValue::Unsigned32(500_000),   // 8: MNConfigurationTimeout_U32
                ObjectValue::Unsigned32(500_000),   // 9: MNStartCNTimeout_U32
            ]),
            name: "NMT_BootTime_REC",
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
        ObjectEntry {
            object: Object::Array(vec![ObjectValue::Unsigned32(0); 254]),
            name: "NMT_MNNodeList_AU32",
            category: Category::Mandatory,
            access: Some(AccessType::ReadWrite),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );

    // 0x1F85: NMT_MNVendorIdList_AU32
    od.insert(
        0x1F85,
        ObjectEntry {
            object: Object::Array(vec![ObjectValue::Unsigned32(0); 254]),
            name: "NMT_MNVendorIdList_AU32",
            category: Category::Mandatory,
            access: Some(AccessType::ReadWrite),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );

    // 0x1F86: NMT_MNProductCodeList_AU32
    od.insert(
        0x1F86,
        ObjectEntry {
            object: Object::Array(vec![ObjectValue::Unsigned32(0); 254]),
            name: "NMT_MNProductCodeList_AU32",
            category: Category::Mandatory,
            access: Some(AccessType::ReadWrite),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );

    // 0x1F87: NMT_MNRevisionNoList_AU32
    od.insert(
        0x1F87,
        ObjectEntry {
            object: Object::Array(vec![ObjectValue::Unsigned32(0); 254]),
            name: "NMT_MNRevisionNoList_AU32",
            category: Category::Mandatory,
            access: Some(AccessType::ReadWrite),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );

    // 0x1F8D: DLL_MNPResPayloadLimitList_AU16
    od.insert(
        0x1F8D,
        ObjectEntry {
            object: Object::Array(vec![ObjectValue::Unsigned16(0); 254]),
            name: "DLL_MNPResPayloadLimitList_AU16",
            category: Category::Mandatory,
            access: Some(AccessType::ReadWrite),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );

    // 0x1F92: DLL_MNPResTimeOut_AU32
    od.insert(
        0x1F92,
        ObjectEntry {
            object: Object::Array(vec![ObjectValue::Unsigned32(100000); 254]), // 100ms
            name: "DLL_MNPResTimeOut_AU32",
            category: Category::Mandatory,
            access: Some(AccessType::ReadWrite),
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );

    Ok(od)
}

/// Helper to add standard diagnostic objects (0x1101, 0x1102) to an OD.
fn add_diagnostic_objects(od: &mut ObjectDictionary<'static>) -> Result<(), PowerlinkError> {
    // 0x1101: DIA_NMTTelegrCount_REC (DS 301, 8.1.1)
    od.insert(
        0x1101,
        ObjectEntry {
            object: Object::Record(vec![
                // Sub-index 0 (Count) is implicit.
                ObjectValue::Unsigned32(0), // 1: IsochrCyc_U32
                ObjectValue::Unsigned32(0), // 2: IsochrRx_U32
                ObjectValue::Unsigned32(0), // 3: IsochrTx_U32
                ObjectValue::Unsigned32(0), // 4: AsyncRx_U32
                ObjectValue::Unsigned32(0), // 5: AsyncTx_U32
                ObjectValue::Unsigned32(0), // 6: SdoRx_U32
                ObjectValue::Unsigned32(0), // 7: SdoTx_U32
                ObjectValue::Unsigned32(0), // 8: Status_U32
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
                // Sub-index 0 (Count) is implicit.
                ObjectValue::Unsigned32(0), // 1: HistoryEntryWrite_U32
                ObjectValue::Unsigned32(0), // 2: EmergencyQueueWrite_U32
                ObjectValue::Unsigned32(0), // 3: EmergencyQueueOverflow_U32
                ObjectValue::Unsigned32(0), // 4: StatusEntryChanged_U32
                ObjectValue::Unsigned32(0), // 5: StaticErrorBitFieldChanged_U32
                ObjectValue::Unsigned32(0), // 6: ExceptionResetEdgePos_U32
                ObjectValue::Unsigned32(0), // 7: ExceptionNewEdge_U32
            ]),
            name: "DIA_ERRStatistics_REC",
            category: Category::Optional,
            access: Some(AccessType::ReadOnly),
            default_value: None,
            value_range: None,
            pdo_mapping: Some(PdoMapping::No),
        },
    );

    Ok(())
}