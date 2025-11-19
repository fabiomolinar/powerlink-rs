//! Contains builder functions to convert `types::NetworkManagement` into `model::NetworkManagement`.
//!
//! This module maps the user-facing Network Management configuration (features, diagnostic definitions)
//! back into the schema-compliant internal model for serialization.

use crate::model::common::{Description, Glabels, Label, LabelChoice};
use crate::{model, types};
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Helper to create a `Glabels` struct from optional label and description strings.
///
/// XDC uses `g_labels` to support multilingual text. This builder currently defaults
/// to creating "en" (English) entries for the provided strings.
fn build_glabels(label: Option<&String>, description: Option<&String>) -> Option<Glabels> {
    let mut items = Vec::new();
    if let Some(l) = label {
        items.push(LabelChoice::Label(Label {
            lang: "en".to_string(),
            value: l.clone(),
        }));
    }
    if let Some(d) = description {
        items.push(LabelChoice::Description(Description {
            lang: "en".to_string(),
            value: d.clone(),
            ..Default::default()
        }));
    }
    if items.is_empty() {
        None
    } else {
        Some(Glabels { items })
    }
}

/// Builds `model::net_mgmt::AddInfo` from the public type.
fn build_model_add_info(public: &types::AddInfo) -> model::net_mgmt::AddInfo {
    model::net_mgmt::AddInfo {
        name: public.name.clone(),
        bit_offset: public.bit_offset.to_string(),
        len: public.len.to_string(),
        labels: build_glabels(None, public.description.as_ref()),
        value: Vec::new(), // AddInfoValue support is not yet exposed in public types
    }
}

/// Builds `model::net_mgmt::Error` from the public `ErrorDefinition`.
fn build_model_error(public: &types::ErrorDefinition) -> model::net_mgmt::Error {
    model::net_mgmt::Error {
        name: public.name.clone(),
        value: public.value.clone(),
        labels: None, // ErrorDefinition in types doesn't currently support labels/desc
        add_info: public.add_info.iter().map(build_model_add_info).collect(),
    }
}

/// Builds `model::net_mgmt::ErrorBit` from the public `StaticErrorBit`.
fn build_model_error_bit(public: &types::StaticErrorBit) -> model::net_mgmt::ErrorBit {
    model::net_mgmt::ErrorBit {
        name: public.name.clone(),
        offset: public.offset.to_string(),
        labels: build_glabels(public.label.as_ref(), public.description.as_ref()),
    }
}

/// Builds the `model::net_mgmt::Diagnostic` block.
fn build_model_diagnostic(public: &types::Diagnostic) -> model::net_mgmt::Diagnostic {
    let error_list = if public.errors.is_empty() {
        None
    } else {
        Some(model::net_mgmt::ErrorList {
            error: public.errors.iter().map(build_model_error).collect(),
        })
    };

    let static_error_bit_field =
        public
            .static_error_bit_field
            .as_ref()
            .map(|bits| model::net_mgmt::StaticErrorBitField {
                error_bit: bits.iter().map(build_model_error_bit).collect(),
            });

    model::net_mgmt::Diagnostic {
        error_list,
        static_error_bit_field,
    }
}

/// Converts a public `types::NetworkManagement` into a `model::NetworkManagement`.
///
/// This function aggregates General, MN, and CN features into the internal model format.
pub(super) fn build_model_network_management(
    public: &types::NetworkManagement,
) -> model::net_mgmt::NetworkManagement {
    let general_features = model::net_mgmt::GeneralFeatures {
        dll_feature_mn: public.general_features.dll_feature_mn,
        nmt_boot_time_not_active: public.general_features.nmt_boot_time_not_active.to_string(),
        nmt_cycle_time_max: public.general_features.nmt_cycle_time_max.to_string(),
        nmt_cycle_time_min: public.general_features.nmt_cycle_time_min.to_string(),
        nmt_error_entries: public.general_features.nmt_error_entries.to_string(),
        nmt_max_cn_number: public
            .general_features
            .nmt_max_cn_number
            .map(|v| v.to_string()),
        pdo_dynamic_mapping: public.general_features.pdo_dynamic_mapping,
        sdo_client: public.general_features.sdo_client,
        sdo_server: public.general_features.sdo_server,
        sdo_support_asnd: public.general_features.sdo_support_asnd,
        sdo_support_udp_ip: public.general_features.sdo_support_udp_ip,

        // --- Detailed Feature Flags ---
        nmt_isochronous: public.general_features.nmt_isochronous,
        sdo_support_pdo: public.general_features.sdo_support_pdo,
        nmt_ext_nmt_cmds: public.general_features.nmt_ext_nmt_cmds,
        cfm_config_manager: public.general_features.cfm_config_manager,
        nmt_node_id_by_sw: public.general_features.nmt_node_id_by_sw,
        sdo_cmd_read_all_by_index: public.general_features.sdo_cmd_read_all_by_index,
        sdo_cmd_write_all_by_index: public.general_features.sdo_cmd_write_all_by_index,
        sdo_cmd_read_mult_param: public.general_features.sdo_cmd_read_mult_param,
        sdo_cmd_write_mult_param: public.general_features.sdo_cmd_write_mult_param,
        nmt_publish_active_nodes: public.general_features.nmt_publish_active_nodes,
        nmt_publish_config_nodes: public.general_features.nmt_publish_config_nodes,

        ..Default::default()
    };

    let mn_features = public
        .mn_features
        .as_ref()
        .map(|mnf| model::net_mgmt::MnFeatures {
            dll_mn_feature_multiplex: mnf.dll_mn_feature_multiplex,
            dll_mn_pres_chaining: mnf.dll_mn_pres_chaining,
            nmt_simple_boot: mnf.nmt_simple_boot,
            nmt_service_udp_ip: mnf.nmt_service_udp_ip,
            nmt_mn_basic_ethernet: mnf.nmt_mn_basic_ethernet,
            ..Default::default()
        });

    let cn_features = public
        .cn_features
        .as_ref()
        .map(|cnf| model::net_mgmt::CnFeatures {
            dll_cn_feature_multiplex: cnf.dll_cn_feature_multiplex,
            dll_cn_pres_chaining: cnf.dll_cn_pres_chaining,
            nmt_cn_pre_op2_to_ready2_op: cnf.nmt_cn_pre_op2_to_ready2_op.map(|v| v.to_string()),
            nmt_cn_soc_2_preq: cnf.nmt_cn_soc_2_preq.to_string(),
            nmt_cn_dna: cnf.nmt_cn_dna.map(|dna| match dna {
                types::NmtCnDna::DoNotClear => model::net_mgmt::CnFeaturesNmtCnDna::DoNotClear,
                types::NmtCnDna::ClearOnPreOp1ToPreOp2 => {
                    model::net_mgmt::CnFeaturesNmtCnDna::ClearOnPreOp1ToPreOp2
                }
                types::NmtCnDna::ClearOnNmtResetNode => {
                    model::net_mgmt::CnFeaturesNmtCnDna::ClearOnNmtResetNode
                }
            }),
            ..Default::default()
        });

    let diagnostic = public.diagnostic.as_ref().map(build_model_diagnostic);

    model::net_mgmt::NetworkManagement {
        general_features,
        mn_features,
        cn_features,
        diagnostic,
        device_commissioning: None, // DeviceCommissioning is typically not round-tripped this way
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec;

    use super::*;
    use crate::types;

    #[test]
    fn test_build_model_network_management() {
        // 1. Create public types
        let public_nm = types::NetworkManagement {
            general_features: types::GeneralFeatures {
                dll_feature_mn: true,
                nmt_boot_time_not_active: 50000,
                nmt_cycle_time_max: 10000,
                nmt_cycle_time_min: 200,
                nmt_error_entries: 5,
                nmt_max_cn_number: Some(10),
                pdo_dynamic_mapping: Some(true),
                sdo_server: Some(true),
                ..Default::default()
            },
            mn_features: Some(types::MnFeatures {
                dll_mn_feature_multiplex: Some(true),
                dll_mn_pres_chaining: Some(false),
                nmt_simple_boot: true,
                ..Default::default()
            }),
            cn_features: Some(types::CnFeatures {
                dll_cn_feature_multiplex: Some(false),
                nmt_cn_soc_2_preq: 1200,
                nmt_cn_dna: Some(types::NmtCnDna::ClearOnNmtResetNode),
                ..Default::default()
            }),
            diagnostic: Some(types::Diagnostic {
                errors: vec![types::ErrorDefinition {
                    name: "TestError".to_string(),
                    value: "0x1000".to_string(),
                    add_info: vec![],
                }],
                static_error_bit_field: None,
            }),
        };

        // 2. Call the builder
        let model_nm = build_model_network_management(&public_nm);

        // 3. Verify GeneralFeatures
        let model_gf = &model_nm.general_features;
        assert_eq!(model_gf.dll_feature_mn, true);
        assert_eq!(model_gf.nmt_boot_time_not_active, "50000");
        assert_eq!(model_gf.nmt_cycle_time_max, "10000");
        assert_eq!(model_gf.nmt_cycle_time_min, "200");
        assert_eq!(model_gf.nmt_error_entries, "5");
        assert_eq!(model_gf.nmt_max_cn_number, Some("10".to_string()));
        assert_eq!(model_gf.pdo_dynamic_mapping, Some(true));
        assert_eq!(model_gf.sdo_server, Some(true));

        // 4. Verify MNFeatures
        let model_mnf = model_nm.mn_features.unwrap();
        assert_eq!(model_mnf.dll_mn_feature_multiplex, Some(true));
        assert_eq!(model_mnf.dll_mn_pres_chaining, Some(false));
        assert_eq!(model_mnf.nmt_simple_boot, true);

        // 5. Verify CNFeatures
        let model_cnf = model_nm.cn_features.unwrap();
        assert_eq!(model_cnf.dll_cn_feature_multiplex, Some(false));
        assert_eq!(model_cnf.nmt_cn_soc_2_preq, "1200");
        assert_eq!(
            model_cnf.nmt_cn_dna,
            Some(model::net_mgmt::CnFeaturesNmtCnDna::ClearOnNmtResetNode)
        );
    }
}