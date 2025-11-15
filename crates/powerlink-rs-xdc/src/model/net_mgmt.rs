// crates/powerlink-rs-xdc/src/model/net_mgmt.rs

//! Contains model structs related to `<NetworkManagement>`.
//! (Schema: `ProfileBody_CommunicationNetwork_Powerlink.xsd`)

use serde::{Deserialize, Serialize};
use alloc::vec::Vec;
use alloc::string::String;

/// Represents `<NetworkManagement>` (from XSD `t_NetworkManagement`).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct NetworkManagement {
    #[serde(rename = "GeneralFeatures")]
    pub general_features: GeneralFeatures,
    
    #[serde(rename = "MNFeatures", default, skip_serializing_if = "Option::is_none")]
    pub mn_features: Option<MnFeatures>,
    
    #[serde(rename = "CNFeatures", default, skip_serializing_if = "Option::is_none")]
    pub cn_features: Option<CnFeatures>,
    
    #[serde(rename = "deviceCommissioning", default, skip_serializing_if = "Option::is_none")]
    pub device_commissioning: Option<DeviceCommissioning>,
    
    #[serde(rename = "Diagnostic", default, skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<Diagnostic>,
}

/// Represents `<GeneralFeatures>` (from XSD `t_GeneralFeatures`).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct GeneralFeatures {
    #[serde(rename = "@DLLFeatureMN", default, skip_serializing_if = "Option::is_none")]
    pub dll_feature_mn: Option<bool>,
    
    #[serde(rename = "@NMTBootTimeNotActive", default, skip_serializing_if = "Option::is_none")]
    pub nmt_boot_time_not_active: Option<String>,
    
    // ... other GeneralFeatures attributes can be added here ...
}

/// Represents `<MNFeatures>` (from XSD `t_MNFeatures`).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct MnFeatures {
    #[serde(rename = "@NMTMNMaxCycInSync", default, skip_serializing_if = "Option::is_none")]
    pub nmt_mn_max_cyc_in_sync: Option<String>,
    
    #[serde(rename = "@NMTMNPResMax", default, skip_serializing_if = "Option::is_none")]
    pub nmt_mn_pres_max: Option<String>,
    
    // ... other MNFeatures attributes can be added here ...
}

/// Represents the `NMTCNDNA` attribute enum (from XSD `t_CNFeaturesNMT_CN_DNA`).
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum CnFeaturesNmtCnDna {
    /// "0" = Do not clear
    #[serde(rename = "0")]
    DoNotClear,
    /// "1" = Clear on PRE_OP1 -> PRE_OP2
    #[serde(rename = "1")]
    ClearOnPreOp1ToPreOp2,
    /// "2" = Clear on NMT_Reset_Node
    #[serde(rename = "2")]
    ClearOnNmtResetNode,
}

/// Represents `<CNFeatures>` (from XSD `t_CNFeatures`).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct CnFeatures {
    #[serde(rename = "@NMTCNPreOp2ToReady2Op", default, skip_serializing_if = "Option::is_none")]
    pub nmt_cn_pre_op2_to_ready2_op: Option<String>,
    
    #[serde(rename = "@NMTCNDNA", default, skip_serializing_if = "Option::is_none")]
    pub nmt_cn_dna: Option<CnFeaturesNmtCnDna>,
    
    // ... other CNFeatures attributes can be added here ...
}

/// Represents `<deviceCommissioning>` (from XSD `t_deviceCommissioning`).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct DeviceCommissioning {
    #[serde(rename = "@NMTNodeIDByHW", default)]
    pub nmt_node_id_by_hw: bool,

    #[serde(rename = "@NMTNodeIDBySW", default)]
    pub nmt_node_id_by_sw: bool,
    
    // ... other deviceCommissioning attributes can be added here ...
}

/// Represents `<Diagnostic>` (from XSD `t_Diagnostic`).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Diagnostic {
    #[serde(rename = "ErrorList", default, skip_serializing_if = "Option::is_none")]
    pub error_list: Option<ErrorList>,
}

/// Represents `<ErrorList>` (from XSD `t_ErrorList`).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ErrorList {
    #[serde(rename = "Error", default, skip_serializing_if = "Vec::is_empty")]
    pub error: Vec<Error>,
}

/// Represents `<Error>` (from XSD `t_Error`).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Error {
    #[serde(rename = "@name", default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    // Note: The schema in `powerlink-rs-xdc.md` for `t_Error` is simplified
    // and doesn't match the XSD in `ProfileBody_CommunicationNetwork_Powerlink.xsd`.
    // The XSD defines a complex `ErrorConstant_DataType`.
    // For now, we stick to the simplified `model.rs` definition.
    #[serde(rename = "@label", default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    
    #[serde(rename = "@description", default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    
    #[serde(rename = "@type", default, skip_serializing_if = "Option::is_none")]
    pub error_type: Option<String>,
    
    #[serde(rename = "@value", default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}