use super::ObjectDictionary;
use super::entry::{AccessType, Category, Object, ObjectEntry, PdoMapping};
use super::value::ObjectValue;
use crate::PowerlinkError;
use alloc::vec;

/// Populates the OD with mandatory objects that define protocol mechanisms.
/// Device-specific identification objects are left to the user to insert.
pub(super) fn populate_protocol_objects(od: &mut ObjectDictionary) {
    // Add "Store Parameters" (1010h)
    od.insert(
        0x1010,
        ObjectEntry {
            object: Object::Record(vec![
                ObjectValue::Unsigned32(1), // Sub-index 1: Save All Parameters
                ObjectValue::Unsigned32(1), // Sub-index 2: Save Communication Parameters
                ObjectValue::Unsigned32(1), // Sub-index 3: Save Application Parameters
            ]),
            name: "NMT_StoreParam_REC",
            category: Category::Mandatory, // Spec says Optional, but it's fundamental
            access: None,
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );

    // Add "Restore Default Parameters" (1011h)
    od.insert(
        0x1011,
        ObjectEntry {
            object: Object::Record(vec![
                ObjectValue::Unsigned32(1), // Sub-index 1: Restore All Parameters
                ObjectValue::Unsigned32(1), // Sub-index 2: Restore Communication Parameters
                ObjectValue::Unsigned32(1), // Sub-index 3: Restore Application Parameters
            ]),
            name: "NMT_RestoreDefParam_REC",
            category: Category::Optional,
            access: None,
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );

    // Add SDO Sequence Layer Timeout (1300h)
    od.insert(
        0x1300,
        ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned32(15000)), // Default: 15000ms
            name: "SDO_SequLayerTimeout_U32",
            category: Category::Mandatory,
            access: Some(AccessType::ReadWriteStore),
            default_value: Some(ObjectValue::Unsigned32(15000)),
            value_range: None,
            pdo_mapping: Some(PdoMapping::No),
        },
    );

    // Add SDO Number of Acknowledge Retries (1302h)
    od.insert(
        0x1302,
        ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned32(2)), // Default: 2 retries
            name: "SDO_SequLayerNoAck_U32",
            category: Category::Optional,
            access: Some(AccessType::ReadWriteStore),
            default_value: Some(ObjectValue::Unsigned32(2)),
            value_range: None,
            pdo_mapping: Some(PdoMapping::No),
        },
    );

    // Add "PDO_CommParamRecord_TYPE" (0x0420) definition
    od.insert(
        0x0420,
        ObjectEntry {
            object: Object::Record(vec![
                ObjectValue::Unsigned8(2), // NumberOfEntries
                ObjectValue::Unsigned8(0), // NodeID_U8
                ObjectValue::Unsigned8(0), // MappingVersion_U8
            ]),
            name: "PDO_CommParamRecord_TYPE",
            category: Category::Mandatory, // This is a type definition
            access: Some(AccessType::Constant),
            default_value: None,
            value_range: None,
            pdo_mapping: Some(PdoMapping::No),
        },
    );

    // Add "RPDO Communication Parameter" (1400h) - Default entry
    od.insert(
        0x1400,
        ObjectEntry {
            object: Object::Record(vec![
                ObjectValue::Unsigned8(0), // 1: NodeID_U8 (0 = mapped to PReq)
                ObjectValue::Unsigned8(0), // 2: MappingVersion_U8
            ]),
            name: "PDO_RxCommParam_00h_REC",
            category: Category::Conditional,
            access: None, // Access is per-subindex
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );

    // Add "RPDO Mapping Parameter" (1600h) - Default entry
    od.insert(
        0x1600,
        ObjectEntry {
            object: Object::Array(vec![]), // Empty mapping by default
            name: "PDO_RxMappParam_00h_AU64",
            category: Category::Conditional,
            access: None, // Access is per-subindex
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );

    // Add "TPDO Communication Parameter" (1800h) - Default entry
    od.insert(
        0x1800,
        ObjectEntry {
            object: Object::Record(vec![
                ObjectValue::Unsigned8(0), // 1: NodeID_U8 (0 = mapped to PRes)
                ObjectValue::Unsigned8(0), // 2: MappingVersion_U8
            ]),
            name: "PDO_TxCommParam_00h_REC",
            category: Category::Conditional,
            access: None, // Access is per-subindex
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );

    // Add "TPDO Mapping Parameter" (1A00h) - Default entry
    od.insert(
        0x1A00,
        ObjectEntry {
            object: Object::Array(vec![]), // Empty mapping by default
            name: "PDO_TxMappParam_00h_AU64",
            category: Category::Conditional,
            access: None, // Access is per-subindex
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );

    // Add "PDO_ErrMapVers_OSTR" (1C80h) - Optional error logging
    od.insert(
        0x1C80,
        ObjectEntry {
            object: Object::Variable(ObjectValue::OctetString(vec![0; 32])),
            name: "PDO_ErrMapVers_OSTR",
            category: Category::Optional,
            access: Some(AccessType::ReadWrite),
            default_value: Some(ObjectValue::OctetString(vec![0; 32])),
            value_range: None,
            pdo_mapping: Some(PdoMapping::No),
        },
    );

    // Add "PDO_ErrShort_RX_OSTR" (1C81h) - Optional error logging
    od.insert(
        0x1C81,
        ObjectEntry {
            object: Object::Variable(ObjectValue::OctetString(vec![0; 32])),
            name: "PDO_ErrShort_RX_OSTR",
            category: Category::Optional,
            access: Some(AccessType::ReadWrite),
            default_value: Some(ObjectValue::OctetString(vec![0; 32])),
            value_range: None,
            pdo_mapping: Some(PdoMapping::No),
        },
    );

    // Add "NMT_CurrNMTState_U8" (1F8Ch)
    od.insert(
        0x1F8C,
        ObjectEntry {
            object: Object::Variable(ObjectValue::Unsigned8(0)),
            name: "NMT_CurrNMTState_U8",
            category: Category::Mandatory,
            access: Some(AccessType::ReadOnly),
            default_value: Some(ObjectValue::Unsigned8(0)),
            value_range: None,
            pdo_mapping: Some(PdoMapping::No),
        },
    );

    // --- Add default PDO Communication Parameters ---
    // RPDO 1 Comm Param (for PReq from MN, NodeID 0)
    od.insert(
        0x1400,
        ObjectEntry {
            object: Object::Record(vec![
                ObjectValue::Unsigned8(0), // 1: NodeID_U8 (0 = PReq)
                ObjectValue::Unsigned8(0), // 2: MappingVersion_U8
            ]),
            name: "PDO_RxCommParam_00h_REC",
            category: Category::Mandatory,
            access: None,
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );
    // TPDO 1 Comm Param (for PRes from this CN)
    od.insert(
        0x1800,
        ObjectEntry {
            object: Object::Record(vec![
                ObjectValue::Unsigned8(1), // 1: NodeID_U8 (1 = self, placeholder)
                ObjectValue::Unsigned8(0), // 2: MappingVersion_U8
            ]),
            name: "PDO_TxCommParam_00h_REC",
            category: Category::Mandatory,
            access: None,
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );

    // --- Add default PDO Mapping Parameters (empty) ---
    // RPDO 1 Mapping Param
    od.insert(
        0x1600,
        ObjectEntry {
            object: Object::Array(vec![]), // Empty mapping by default
            name: "PDO_RxMappParam_00h_AU64",
            category: Category::Mandatory,
            access: None,
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );
    // TPDO 1 Mapping Param
    od.insert(
        0x1A00,
        ObjectEntry {
            object: Object::Array(vec![]), // Empty mapping by default
            name: "PDO_TxMappParam_00h_AU64",
            category: Category::Mandatory,
            access: None,
            default_value: None,
            value_range: None,
            pdo_mapping: None,
        },
    );
}

/// Validates that the OD contains all mandatory objects required for a node to function.
pub(super) fn validate_mandatory_objects(
    od: &ObjectDictionary,
    is_mn: bool,
) -> Result<(), PowerlinkError> {
    const COMMON_MANDATORY_OBJECTS: &[u16] = &[
        0x1000, // NMT_DeviceType_U32
        0x1018, // NMT_IdentityObject_REC
        0x1F82, // NMT_FeatureFlags_U32
    ];
    for &index in COMMON_MANDATORY_OBJECTS {
        if !od.entries.contains_key(&index) {
            return Err(PowerlinkError::ValidationError(
                "Missing common mandatory object",
            ));
        }
    }

    if is_mn {
        const MN_MANDATORY_OBJECTS: &[u16] = &[
            0x1006, // NMT_CycleLen_U32
            0x1F81, // NMT_NodeAssignment_AU32
            0x1F89, // NMT_BootTime_REC
        ];
        for &index in MN_MANDATORY_OBJECTS {
            if !od.entries.contains_key(&index) {
                return Err(PowerlinkError::ValidationError(
                    "Missing MN-specific mandatory object",
                ));
            }
        }
    } else {
        const CN_MANDATORY_OBJECTS: &[u16] = &[
            0x1F93, // NMT_EPLNodeID_REC
            0x1F99, // NMT_CNBasicEthernetTimeout_U32
        ];
        for &index in CN_MANDATORY_OBJECTS {
            if !od.entries.contains_key(&index) {
                return Err(PowerlinkError::ValidationError(
                    "Missing CN-specific mandatory object",
                ));
            }
        }
    }
    Ok(())
}
