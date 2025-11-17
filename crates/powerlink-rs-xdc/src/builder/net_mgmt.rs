// crates/powerlink-rs-xdc/src/builder/net_mgmt.rs

//! Contains builder functions to convert `types::NetworkManagement` into `model::NetworkManagement`.

use crate::{model, types};
use alloc::string::ToString;

/// Converts a public `types::NetworkManagement` into a `model::NetworkManagement`.
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
        ..Default::default() // Fills in other attributes as default
    };

    let mn_features = public.mn_features.as_ref().map(|mnf| {
        model::net_mgmt::MnFeatures {
            dll_mn_feature_multiplex: mnf.dll_mn_feature_multiplex,
            dll_mn_pres_chaining: mnf.dll_mn_pres_chaining,
            nmt_simple_boot: mnf.nmt_simple_boot,
            ..Default::default() // Fills in required string fields as empty
        }
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

    // TODO: Implement Diagnostic builder
    let diagnostic = None;

    model::net_mgmt::NetworkManagement {
        general_features,
        mn_features,
        cn_features,
        diagnostic,
        device_commissioning: None, // Never serialize this, it's XDC-only
    }
}

#[cfg(test)]
mod tests {
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
            }),
            cn_features: Some(types::CnFeatures {
                dll_cn_feature_multiplex: Some(false),
                nmt_cn_soc_2_preq: 1200,
                nmt_cn_dna: Some(types::NmtCnDna::ClearOnNmtResetNode),
                ..Default::default()
            }),
            diagnostic: None, // Not tested yet
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
