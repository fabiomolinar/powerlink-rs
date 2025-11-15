// crates/powerlink-rs-xdc/src/model/net_mgmt.rs

//! Contains model structs related to `<NetworkManagement>`.
//! (Schema: `ProfileBody_CommunicationNetwork_Powerlink.xsd`)

use serde::{Deserialize, Serialize};
use alloc::vec::Vec;
use alloc::string::String;
use super::common::Glabels; // Import Glabels for Diagnostic types

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
/// This struct now contains all attributes defined in the schema.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct GeneralFeatures {
    // --- Attributes from schema `t_GeneralFeatures` ---
    #[serde(rename = "@CFMConfigManager", default, skip_serializing_if = "Option::is_none")]
    pub cfm_config_manager: Option<bool>,
    #[serde(rename = "@DLLErrBadPhysMode", default, skip_serializing_if = "Option::is_none")]
    pub dll_err_bad_phys_mode: Option<bool>,
    #[serde(rename = "@DLLErrMacBuffer", default, skip_serializing_if = "Option::is_none")]
    pub dll_err_mac_buffer: Option<bool>,
    #[serde(rename = "@DLLFeatureCN", default, skip_serializing_if = "Option::is_none")]
    pub dll_feature_cn: Option<bool>,
    #[serde(rename = "@DLLFeatureMN")] // Required
    pub dll_feature_mn: bool,
    #[serde(rename = "@NMTBootTimeNotActive")] // Required
    pub nmt_boot_time_not_active: String, // xsd:unsignedInt
    #[serde(rename = "@NMTCycleTimeGranularity", default, skip_serializing_if = "Option::is_none")]
    pub nmt_cycle_time_granularity: Option<String>, // xsd:unsignedInt
    #[serde(rename = "@NMTCycleTimeMax")] // Required
    pub nmt_cycle_time_max: String, // xsd:unsignedInt
    #[serde(rename = "@NMTCycleTimeMin")] // Required
    pub nmt_cycle_time_min: String, // xsd:unsignedInt
    #[serde(rename = "@NMTMinRedCycleTime", default, skip_serializing_if = "Option::is_none")]
    pub nmt_min_red_cycle_time: Option<String>, // xsd:unsignedInt
    #[serde(rename = "@NMTEmergencyQueueSize", default, skip_serializing_if = "Option::is_none")]
    pub nmt_emergency_queue_size: Option<String>, // xsd:unsignedInt
    #[serde(rename = "@NMTErrorEntries")] // Required
    pub nmt_error_entries: String, // xsd:unsignedInt
    #[serde(rename = "@NMTExtNmtCmds", default, skip_serializing_if = "Option::is_none")]
    pub nmt_ext_nmt_cmds: Option<bool>,
    #[serde(rename = "@NMTFlushArpEntry", default, skip_serializing_if = "Option::is_none")]
    pub nmt_flush_arp_entry: Option<bool>,
    #[serde(rename = "@NMTIsochronous", default, skip_serializing_if = "Option::is_none")]
    pub nmt_isochronous: Option<bool>,
    #[serde(rename = "@NMTNetHostNameSet", default, skip_serializing_if = "Option::is_none")]
    pub nmt_net_host_name_set: Option<bool>,
    #[serde(rename = "@NMTMaxCNNodeID", default, skip_serializing_if = "Option::is_none")]
    pub nmt_max_cn_node_id: Option<String>, // xsd:unsignedByte
    #[serde(rename = "@NMTMaxCNNumber", default, skip_serializing_if = "Option::is_none")]
    pub nmt_max_cn_number: Option<String>, // xsd:unsignedByte
    #[serde(rename = "@NMTMaxHeartbeats", default, skip_serializing_if = "Option::is_none")]
    pub nmt_max_heartbeats: Option<String>, // xsd:unsignedByte
    #[serde(rename = "@NMTNodeIDByHW", default, skip_serializing_if = "Option::is_none")]
    pub nmt_node_id_by_hw: Option<bool>,
    #[serde(rename = "@NMTNodeIDBySW", default, skip_serializing_if = "Option::is_none")]
    pub nmt_node_id_by_sw: Option<bool>,
    #[serde(rename = "@NMTProductCode", default, skip_serializing_if = "Option::is_none")]
    pub nmt_product_code: Option<String>, // xsd:unsignedInt
    #[serde(rename = "@NMTPublishActiveNodes", default, skip_serializing_if = "Option::is_none")]
    pub nmt_publish_active_nodes: Option<bool>,
    #[serde(rename = "@NMTPublishConfigNodes", default, skip_serializing_if = "Option::is_none")]
    pub nmt_publish_config_nodes: Option<bool>,
    #[serde(rename = "@NMTPublishEmergencyNew", default, skip_serializing_if = "Option::is_none")]
    pub nmt_publish_emergency_new: Option<bool>,
    #[serde(rename = "@NMTPublishNodeState", default, skip_serializing_if = "Option::is_none")]
    pub nmt_publish_node_state: Option<bool>,
    #[serde(rename = "@NMTPublishOperational", default, skip_serializing_if = "Option::is_none")]
    pub nmt_publish_operational: Option<bool>,
    #[serde(rename = "@NMTPublishPreOp1", default, skip_serializing_if = "Option::is_none")]
    pub nmt_publish_pre_op1: Option<bool>,
    #[serde(rename = "@NMTPublishPreOp2", default, skip_serializing_if = "Option::is_none")]
    pub nmt_publish_pre_op2: Option<bool>,
    #[serde(rename = "@NMTPublishReadyToOp", default, skip_serializing_if = "Option::is_none")]
    pub nmt_publish_ready_to_op: Option<bool>,
    #[serde(rename = "@NMTPublishStopped", default, skip_serializing_if = "Option::is_none")]
    pub nmt_publish_stopped: Option<bool>,
    #[serde(rename = "@NMTPublishTime", default, skip_serializing_if = "Option::is_none")]
    pub nmt_publish_time: Option<bool>,
    #[serde(rename = "@NMTRevisionNo", default, skip_serializing_if = "Option::is_none")]
    pub nmt_revision_no: Option<String>, // xsd:unsignedInt
    #[serde(rename = "@NWLForward", default, skip_serializing_if = "Option::is_none")]
    pub nwl_forward: Option<bool>,
    #[serde(rename = "@NWLICMPSupport", default, skip_serializing_if = "Option::is_none")]
    pub nwl_icmp_support: Option<bool>,
    #[serde(rename = "@NWLIPSupport", default, skip_serializing_if = "Option::is_none")]
    pub nwl_ip_support: Option<bool>,
    #[serde(rename = "@PDODynamicMapping", default, skip_serializing_if = "Option::is_none")]
    pub pdo_dynamic_mapping: Option<bool>,
    #[serde(rename = "@PDOGranularity", default, skip_serializing_if = "Option::is_none")]
    pub pdo_granularity: Option<String>, // xsd:unsignedByte
    #[serde(rename = "@PDOMaxDescrMem", default, skip_serializing_if = "Option::is_none")]
    pub pdo_max_descr_mem: Option<String>, // xsd:unsignedInt
    #[serde(rename = "@PDORPDOChannelObjects", default, skip_serializing_if = "Option::is_none")]
    pub pdo_rpdo_channel_objects: Option<String>, // xsd:unsignedByte
    #[serde(rename = "@PDORPDOChannels", default, skip_serializing_if = "Option::is_none")]
    pub pdo_rpdo_channels: Option<String>, // xsd:unsignedShort
    #[serde(rename = "@PDORPDOCycleDataLim", default, skip_serializing_if = "Option::is_none")]
    pub pdo_rpdo_cycle_data_lim: Option<String>, // xsd:unsignedInt
    #[serde(rename = "@PDORPDOOverallObjects", default, skip_serializing_if = "Option::is_none")]
    pub pdo_rpdo_overall_objects: Option<String>, // xsd:unsignedShort
    #[serde(rename = "@PDOSelfReceipt", default, skip_serializing_if = "Option::is_none")]
    pub pdo_self_receipt: Option<bool>,
    #[serde(rename = "@PDOTPDOChannelObjects", default, skip_serializing_if = "Option::is_none")]
    pub pdo_tpdo_channel_objects: Option<String>, // xsd:unsignedByte
    #[serde(rename = "@PDOTPDOCycleDataLim", default, skip_serializing_if = "Option::is_none")]
    pub pdo_tpdo_cycle_data_lim: Option<String>, // xsd:unsignedInt
    #[serde(rename = "@PDOTPDOOverallObjects", default, skip_serializing_if = "Option::is_none")]
    pub pdo_tpdo_overall_objects: Option<String>, // xsd:unsignedShort
    #[serde(rename = "@PHYExtEPLPorts", default, skip_serializing_if = "Option::is_none")]
    pub phy_ext_epl_ports: Option<String>, // xsd:unsignedByte
    #[serde(rename = "@PHYHubDelay", default, skip_serializing_if = "Option::is_none")]
    pub phy_hub_delay: Option<String>, // xsd:unsignedInt
    #[serde(rename = "@PHYHubIntegrated", default, skip_serializing_if = "Option::is_none")]
    pub phy_hub_integrated: Option<bool>,
    #[serde(rename = "@PHYHubJitter", default, skip_serializing_if = "Option::is_none")]
    pub phy_hub_jitter: Option<String>, // xsd:unsignedInt
    #[serde(rename = "@RT1RT1SecuritySupport", default, skip_serializing_if = "Option::is_none")]
    pub rt1_rt1_security_support: Option<bool>,
    #[serde(rename = "@RT1RT1Support", default, skip_serializing_if = "Option::is_none")]
    pub rt1_rt1_support: Option<bool>,
    #[serde(rename = "@RT2RT2Support", default, skip_serializing_if = "Option::is_none")]
    pub rt2_rt2_support: Option<bool>,
    #[serde(rename = "@SDOClient", default, skip_serializing_if = "Option::is_none")]
    pub sdo_client: Option<bool>,
    #[serde(rename = "@SDOCmdFileRead", default, skip_serializing_if = "Option::is_none")]
    pub sdo_cmd_file_read: Option<bool>,
    #[serde(rename = "@SDOCmdFileWrite", default, skip_serializing_if = "Option::is_none")]
    pub sdo_cmd_file_write: Option<bool>,
    #[serde(rename = "@SDOCmdLinkName", default, skip_serializing_if = "Option::is_none")]
    pub sdo_cmd_link_name: Option<bool>,
    #[serde(rename = "@SDOCmdReadAllByIndex", default, skip_serializing_if = "Option::is_none")]
    pub sdo_cmd_read_all_by_index: Option<bool>,
    #[serde(rename = "@SDOCmdReadByName", default, skip_serializing_if = "Option::is_none")]
    pub sdo_cmd_read_by_name: Option<bool>,
    #[serde(rename = "@SDOCmdReadMultParam", default, skip_serializing_if = "Option::is_none")]
    pub sdo_cmd_read_mult_param: Option<bool>,
    #[serde(rename = "@SDOCmdWriteAllByIndex", default, skip_serializing_if = "Option::is_none")]
    pub sdo_cmd_write_all_by_index: Option<bool>,
    #[serde(rename = "@SDOCmdWriteByName", default, skip_serializing_if = "Option::is_none")]
    pub sdo_cmd_write_by_name: Option<bool>,
    #[serde(rename = "@SDOCmdWriteMultParam", default, skip_serializing_if = "Option::is_none")]
    pub sdo_cmd_write_mult_param: Option<bool>,
    #[serde(rename = "@SDOMaxConnections", default, skip_serializing_if = "Option::is_none")]
    pub sdo_max_connections: Option<String>, // xsd:unsignedInt
    #[serde(rename = "@SDOMaxParallelConnections", default, skip_serializing_if = "Option::is_none")]
    pub sdo_max_parallel_connections: Option<String>, // xsd:unsignedInt
    #[serde(rename = "@SDOSeqLayerTxHistorySize", default, skip_serializing_if = "Option::is_none")]
    pub sdo_seq_layer_tx_history_size: Option<String>, // xsd:unsignedShort
    #[serde(rename = "@SDOServer", default, skip_serializing_if = "Option::is_none")]
    pub sdo_server: Option<bool>,
    #[serde(rename = "@SDOSupportASnd", default, skip_serializing_if = "Option::is_none")]
    pub sdo_support_asnd: Option<bool>,
    #[serde(rename = "@SDOSupportPDO", default, skip_serializing_if = "Option::is_none")]
    pub sdo_support_pdo: Option<bool>,
    #[serde(rename = "@SDOSupportUdpIp", default, skip_serializing_if = "Option::is_none")]
    pub sdo_support_udp_ip: Option<bool>,
    #[serde(rename = "@DLLMultiplePReqPRes", default, skip_serializing_if = "Option::is_none")]
    pub dll_multiple_preq_pres: Option<bool>,
}

/// Represents `<MNFeatures>` (from XSD `t_MNFeatures`).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct MnFeatures {
    // --- Attributes from schema `t_MNFeatures` ---
    #[serde(rename = "@DLLErrMNMultipleMN", default, skip_serializing_if = "Option::is_none")]
    pub dll_err_mn_multiple_mn: Option<bool>,
    #[serde(rename = "@DLLMNFeatureMultiplex", default, skip_serializing_if = "Option::is_none")]
    pub dll_mn_feature_multiplex: Option<bool>,
    #[serde(rename = "@DLLMNPResChaining", default, skip_serializing_if = "Option::is_none")]
    pub dll_mn_pres_chaining: Option<bool>,
    #[serde(rename = "@DLLMNFeaturePResTx", default, skip_serializing_if = "Option::is_none")]
    pub dll_mn_feature_pres_tx: Option<bool>,
    #[serde(rename = "@NMTMNASnd2SoC")] // Required
    pub nmt_mn_asnd_2_soc: String, // xsd:unsignedInt
    #[serde(rename = "@NMTMNBasicEthernet", default, skip_serializing_if = "Option::is_none")]
    pub nmt_mn_basic_ethernet: Option<bool>,
    #[serde(rename = "@NMTMNMultiplCycMax", default, skip_serializing_if = "Option::is_none")]
    pub nmt_mn_multipl_cyc_max: Option<String>, // xsd:unsignedByte
    #[serde(rename = "@NMTMNPRes2PReq")] // Required
    pub nmt_mn_pres_2_preq: String, // xsd:unsignedInt
    #[serde(rename = "@NMTMNPRes2PRes")] // Required
    pub nmt_mn_pres_2_pres: String, // xsd:unsignedInt
    #[serde(rename = "@NMTMNPResRx2SoA")] // Required
    pub nmt_mn_pres_rx_2_soa: String, // xsd:unsignedInt
    #[serde(rename = "@NMTMNPResTx2SoA")] // Required
    pub nmt_mn_pres_tx_2_soa: String, // xsd:unsignedInt
    #[serde(rename = "@NMTMNSoA2ASndTx")] // Required
    pub nmt_mn_soa_2_asnd_tx: String, // xsd:unsignedInt
    #[serde(rename = "@NMTMNSoC2PReq")] // Required
    pub nmt_mn_soc_2_preq: String, // xsd:unsignedInt
    #[serde(rename = "@NMTNetTime", default, skip_serializing_if = "Option::is_none")]
    pub nmt_net_time: Option<bool>,
    #[serde(rename = "@NMTNetTimeIsRealTime", default, skip_serializing_if = "Option::is_none")]
    pub nmt_net_time_is_real_time: Option<bool>,
    #[serde(rename = "@NMTRelativeTime", default, skip_serializing_if = "Option::is_none")]
    pub nmt_relative_time: Option<bool>,
    #[serde(rename = "@NMTServiceUdpIp", default, skip_serializing_if = "Option::is_none")]
    pub nmt_service_udp_ip: Option<bool>,
    #[serde(rename = "@NMTSimpleBoot")] // Required
    pub nmt_simple_boot: bool,
    #[serde(rename = "@PDOTPDOChannels", default, skip_serializing_if = "Option::is_none")]
    pub pdo_tpdo_channels: Option<String>, // xsd:unsignedShort
    #[serde(rename = "@NMTMNDNA", default, skip_serializing_if = "Option::is_none")]
    pub nmt_mn_dna: Option<bool>,
    #[serde(rename = "@NMTMNRedundancy", default, skip_serializing_if = "Option::is_none")]
    pub nmt_mn_redundancy: Option<bool>,
    #[serde(rename = "@DLLMNRingRedundancy", default, skip_serializing_if = "Option::is_none")]
    pub dll_mn_ring_redundancy: Option<bool>,
    #[serde(rename = "@NMTMNMaxAsynchronousSlots", default, skip_serializing_if = "Option::is_none")]
    pub nmt_mn_max_asynchronous_slots: Option<String>, // xsd:unsignedByte
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
    // --- Attributes from schema `t_CNFeatures` ---
    #[serde(rename = "@DLLCNFeatureMultiplex", default, skip_serializing_if = "Option::is_none")]
    pub dll_cn_feature_multiplex: Option<bool>,
    #[serde(rename = "@DLLCNPResChaining", default, skip_serializing_if = "Option::is_none")]
    pub dll_cn_pres_chaining: Option<bool>,
    #[serde(rename = "@NMTCNPreOp2ToReady2Op", default, skip_serializing_if = "Option::is_none")]
    pub nmt_cn_pre_op2_to_ready2_op: Option<String>, // xsd:unsignedInt
    #[serde(rename = "@NMTCNSoC2PReq")] // Required
    pub nmt_cn_soc_2_preq: String, // xsd:unsignedInt
    #[serde(rename = "@NMTCNSetNodeNumberTime", default, skip_serializing_if = "Option::is_none")]
    pub nmt_cn_set_node_number_time: Option<String>, // xsd:unsignedInt
    #[serde(rename = "@NMTCNDNA", default, skip_serializing_if = "Option::is_none")]
    pub nmt_cn_dna: Option<bool>, // Schema shows 'bool' but also an enum? Sticking with bool.
    #[serde(rename = "@NMTCNMaxAInv", default, skip_serializing_if = "Option::is_none")]
    pub nmt_cn_max_ainv: Option<String>, // xsd:unsignedInt
    #[serde(rename = "@DLLCNLossOfSoCToleranceMax", default, skip_serializing_if = "Option::is_none")]
    pub dll_cn_loss_of_soc_tolerance_max: Option<String>, // xsd:unsignedInt
}

/// Represents `<deviceCommissioning>` (from XSD `t_deviceCommissioning`).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct DeviceCommissioning {
    #[serde(rename = "@networkName")]
    pub network_name: String,
    #[serde(rename = "@nodeID")]
    pub node_id: String, // xsd:unsignedByte
    #[serde(rename = "@nodeName")]
    pub node_name: String,
    #[serde(rename = "@nodeType")]
    pub node_type: String, // xsd:NMTOKEN (MN or CN)
    #[serde(rename = "@usedNetworkInterface", default, skip_serializing_if = "Option::is_none")]
    pub used_network_interface: Option<String>, // xsd:unsignedByte
}

/// Represents `<Diagnostic>` (from XSD `t_Diagnostic`).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Diagnostic {
    #[serde(rename = "ErrorList", default, skip_serializing_if = "Option::is_none")]
    pub error_list: Option<ErrorList>,
    #[serde(rename = "StaticErrorBitField", default, skip_serializing_if = "Option::is_none")]
    pub static_error_bit_field: Option<StaticErrorBitField>,
}

/// Represents `<ErrorList>` (from XSD `t_ErrorList`).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ErrorList {
    #[serde(rename = "Error", default, skip_serializing_if = "Vec::is_empty")]
    pub error: Vec<Error>,
}

/// Represents `<Error>` (from XSD `ErrorConstant_DataType`).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Error {
    #[serde(rename = "@name")]
    pub name: String,
    #[serde(rename = "@value")]
    pub value: String,
    #[serde(flatten, default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<Glabels>,
    #[serde(rename = "addInfo", default, skip_serializing_if = "Vec::is_empty")]
    pub add_info: Vec<AddInfo>,
}

/// Represents `<addInfo>` from `ErrorConstant_DataType`.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct AddInfo {
    #[serde(rename = "@bitOffset")]
    pub bit_offset: String, // xsd:unsignedByte
    #[serde(rename = "@len")]
    pub len: String, // xsd:unsignedByte
    #[serde(rename = "@name")]
    pub name: String,
    #[serde(flatten, default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<Glabels>,
    #[serde(rename = "value", default, skip_serializing_if = "Vec::is_empty")]
    pub value: Vec<AddInfoValue>,
}

/// Represents `<value>` from `addInfo`.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct AddInfoValue {
    #[serde(rename = "@value")]
    pub value: String,
    #[serde(rename = "@name")]
    pub name: String,
    #[serde(flatten, default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<Glabels>,
}

/// Represents `<StaticErrorBitField>` from `t_Diagnostic`.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct StaticErrorBitField {
    #[serde(rename = "ErrorBit", default, skip_serializing_if = "Vec::is_empty")]
    pub error_bit: Vec<ErrorBit>,
}

/// Represents `<ErrorBit>` from `ErrorBit_DataType`.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ErrorBit {
    #[serde(rename = "@name")]
    pub name: String,
    #[serde(rename = "@offset")]
    pub offset: String, // xsd:nonNegativeInteger <= 63
    #[serde(flatten, default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<Glabels>,
}