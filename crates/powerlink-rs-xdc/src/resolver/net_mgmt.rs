// crates/powerlink-rs-xdc/src/resolver/net_mgmt.rs

use crate::error::XdcError;
use crate::model;
use crate::types;
use alloc::vec::Vec;

/// Parses a `model::NetworkManagement` into a `types::NetworkManagement`.
pub(super) fn resolve_network_management(
    model: &model::net_mgmt::NetworkManagement,
) -> Result<types::NetworkManagement, XdcError> {
    let general_features = types::GeneralFeatures {
        dll_feature_mn: model.general_features.dll_feature_mn,
        nmt_boot_time_not_active: model.general_features.nmt_boot_time_not_active.clone(),
    };

    let mn_features = model.mn_features.as_ref().map(|mn| types::MnFeatures {
        nmt_mn_max_cyc_in_sync: mn.nmt_mn_max_cyc_in_sync.clone(),
        nmt_mn_pres_max: mn.nmt_mn_pres_max.clone(),
    });

    let cn_features = model.cn_features.as_ref().map(|cn| types::CnFeatures {
        nmt_cn_pre_op2_to_ready2_op: cn.nmt_cn_pre_op2_to_ready2_op.clone(),
        nmt_cn_dna: cn.nmt_cn_dna.map(|dna| dna == model::net_mgmt::CnFeaturesNmtCnDna::ClearOnPreOp1ToPreOp2),
    });

    let diagnostic = model.diagnostic.as_ref().map(resolve_diagnostic).transpose()?;

    Ok(types::NetworkManagement {
        general_features,
        mn_features,
        cn_features,
        diagnostic,
    })
}

/// Parses a `model::Diagnostic` into a `types::Diagnostic`.
fn resolve_diagnostic(model: &model::net_mgmt::Diagnostic) -> Result<types::Diagnostic, XdcError> {
    let errors = model
        .error_list
        .as_ref()
        .map_or(Vec::new(), |list| {
            list.error
                .iter()
                .map(|e| types::ErrorDefinition {
                    name: e.name.clone(),
                    label: e.label.clone(),
                    description: e.description.clone(),
                    error_type: e.error_type.clone(),
                    value: e.value.clone(),
                })
                .collect()
        });

    Ok(types::Diagnostic { errors })
}