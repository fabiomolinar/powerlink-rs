// crates/powerlink-rs/src/od/constants.rs
//! Central repository for standard Object Dictionary indices and sub-indices.
//!
//! This module provides `pub const` definitions for well-known object
//! indices from the POWERLINK specification (e.g., EPSG DS 301),
//! using a consistent `IDX_` and `SUBIDX_` naming convention.

// --- 0x1000 - 0x1FFF: Communication Profile Area ---

// 0x10xx: General Communication
pub const IDX_NMT_DEVICE_TYPE_U32: u16 = 0x1000;
pub const IDX_NMT_ERROR_REGISTER_U8: u16 = 0x1001;
pub const IDX_NMT_CYCLE_LEN_U32: u16 = 0x1006;
pub const IDX_NMT_MANUFACT_DEV_NAME_VS: u16 = 0x1008;
pub const IDX_NMT_STORE_PARAM_CMD_REC: u16 = 0x1010;
pub const IDX_NMT_RESTORE_PARAM_CMD_REC: u16 = 0x1011;
pub const IDX_NMT_IDENTITY_OBJECT_REC: u16 = 0x1018;
pub const IDX_CFM_VERIFY_CONFIG_REC: u16 = 0x1020;

// 0x11xx: Diagnostic Objects
pub const IDX_DIAG_NMT_TELEGR_COUNT_REC: u16 = 0x1101;
pub const SUBIDX_DIAG_NMT_COUNT_ISOCHR_CYC: u8 = 1;
pub const SUBIDX_DIAG_NMT_COUNT_ISOCHR_RX: u8 = 2;
pub const SUBIDX_DIAG_NMT_COUNT_ISOCHR_TX: u8 = 3;
pub const SUBIDX_DIAG_NMT_COUNT_ASYNC_RX: u8 = 4;
pub const SUBIDX_DIAG_NMT_COUNT_ASYNC_TX: u8 = 5;
pub const SUBIDX_DIAG_NMT_COUNT_SDO_RX: u8 = 6;
pub const SUBIDX_DIAG_NMT_COUNT_SDO_TX: u8 = 7;
pub const SUBIDX_DIAG_NMT_COUNT_STATUS_REQ: u8 = 8;

pub const IDX_DIAG_ERR_STATISTICS_REC: u16 = 0x1102;
pub const SUBIDX_DIAG_ERR_STATS_NR_OF_ENTRIES: u8 = 0; // NumberOfEntries (U8)
pub const SUBIDX_DIAG_ERR_STATS_HIST_WRITE: u8 = 1; // HistoryWrite_U32
pub const SUBIDX_DIAG_ERR_STATS_EMCY_WRITE: u8 = 2; // EmergencyQueueWrite_U32
pub const SUBIDX_DIAG_ERR_STATS_EMCY_OVERFLOW: u8 = 3; // EmergencyQueueOverflow_U32
pub const SUBIDX_DIAG_ERR_STATS_STATIC_ERR_CHG: u8 = 5; // StaticErrorBitFieldChanged_U32
pub const SUBIDX_DIAG_ERR_STATS_ER_POS_EDGE: u8 = 6; // ERPositiveEdge_U32
pub const SUBIDX_DIAG_ERR_STATS_EN_EDGE: u8 = 7; // ENEdge_U32

// 0x14xx: RPDO Communication Parameters
pub const IDX_RPDO_COMM_PARAM_REC_START: u16 = 0x1400;
pub const IDX_RPDO_COMM_PARAM_REC_END: u16 = 0x14FF;
pub const IDX_RPDO_COMM_PARAM_REC_1: u16 = 0x1401;

// 0x16xx: RPDO Mapping Parameters
pub const IDX_RPDO_MAPPING_PARAM_REC_START: u16 = 0x1600;
pub const IDX_RPDO_MAPPING_PARAM_REC_END: u16 = 0x16FF;
pub const IDX_RPDO_MAPPING_PARAM_REC_1: u16 = 0x1601;

// 0x18xx: TPDO Communication Parameters
pub const IDX_TPDO_COMM_PARAM_REC_START: u16 = 0x1800;
pub const IDX_TPDO_COMM_PARAM_REC_END: u16 = 0x18FF;
pub const IDX_TPDO_COMM_PARAM_REC_1: u16 = 0x1801;

// 0x1Axx: TPDO Mapping Parameters
pub const IDX_TPDO_MAPPING_PARAM_REC_START: u16 = 0x1A00;
pub const IDX_TPDO_MAPPING_PARAM_REC_END: u16 = 0x1AFF;
pub const IDX_TPDO_MAPPING_PARAM_REC_1: u16 = 0x1A01;

// 0x1Cxx: DLL Parameters
pub const IDX_DLL_CN_LOSS_OF_SOC_TOL_U32: u16 = 0x1C14;

// 0x1Exx: Network Layer Parameters
pub const IDX_NWL_IP_ADDR_TABLE_REC: u16 = 0x1E40;

// 0x1Fxx: NMT Parameters
pub const IDX_NMT_START_UP_U32: u16 = 0x1F80;
pub const IDX_NMT_NODE_ASSIGNMENT_AU32: u16 = 0x1F81;
pub const IDX_NMT_FEATURE_FLAGS_U32: u16 = 0x1F82;
pub const IDX_NMT_EPL_VERSION_U8: u16 = 0x1F83;
pub const IDX_NMT_MN_NODE_CURR_MAC_ADDR_AU8: u16 = 0x1F84;
pub const IDX_NMT_BOOT_TIME_REC: u16 = 0x1F89;
pub const IDX_NMT_MN_CYCLE_TIMING_REC: u16 = 0x1F8A; // MN-specific cycle timing
pub const SUBIDX_NMT_MN_CYCLE_TIMING_ASYNC_SLOT_U32: u8 = 2;
pub const IDX_NMT_MN_PREQ_PAYLOAD_LIMIT_AU16: u16 = 0x1F8B;
pub const IDX_NMT_CURR_NMT_STATE_U8: u16 = 0x1F8C;
pub const IDX_NMT_PRES_PAYLOAD_LIMIT_AU16: u16 = 0x1F8D;
pub const IDX_NMT_MN_CN_PRES_TIMEOUT_AU32: u16 = 0x1F92;
pub const IDX_NMT_EPL_NODE_ID_REC: u16 = 0x1F93;
pub const IDX_NMT_CYCLE_TIMING_REC: u16 = 0x1F98; // General cycle timing
pub const SUBIDX_NMT_CYCLE_TIMING_ISOCHR_TX_MAX_U16: u8 = 1;
pub const SUBIDX_NMT_CYCLE_TIMING_ISOCHR_RX_MAX_U16: u8 = 2;
pub const SUBIDX_NMT_CYCLE_TIMING_PRES_MAX_LATENCY_U32: u8 = 3;
pub const SUBIDX_NMT_CYCLE_TIMING_PREQ_ACT_PAYLOAD_U16: u8 = 4;
pub const SUBIDX_NMT_CYCLE_TIMING_PRES_ACT_PAYLOAD_U16: u8 = 5;
pub const SUBIDX_NMT_CYCLE_TIMING_ASND_MAX_LATENCY_U32: u8 = 6;
pub const SUBIDX_NMT_CYCLE_TIMING_MULT_CYCLE_CNT_U8: u8 = 7;
pub const SUBIDX_NMT_CYCLE_TIMING_ASYNC_MTU_U16: u8 = 8;
pub const SUBIDX_NMT_CYCLE_TIMING_PRESCALER_U16: u8 = 9;
pub const IDX_NMT_CN_BASIC_ETH_TIMEOUT_U32: u16 = 0x1F99;
pub const IDX_NMT_HOST_NAME_VSTR: u16 = 0x1F9A;
pub const IDX_NMT_MULTIPLEX_ASSIGN_REC: u16 = 0x1F9B;

// --- 0x6000 - 0x9FFF: Manufacturer Specific Profile Area ---
// (Used in io_module example)
pub const IDX_IO_DIGITAL_INPUTS_U8: u16 = 0x6000;
pub const IDX_IO_ANALOG_INPUTS_AU16: u16 = 0x6001;
pub const IDX_IO_DIGITAL_OUTPUTS_U8: u16 = 0x6200;
pub const IDX_IO_ANALOG_OUTPUTS_AU16: u16 = 0x6201;