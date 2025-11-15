// crates/powerlink-rs-xdc/src/resolver/net_mgmt.rs

use crate::error::XdcError;
use crate::model;
use crate::types;
use alloc::string::String;
use alloc::vec::Vec;
use crate::model::common::{Glabels, LabelChoice}; // Import label helpers

/// Helper to parse a string attribute as a u32.
fn parse_u32_attr(s: Option<String>) -> u32 {
    s.and_then(|val| val.parse().ok()).unwrap_or(0)
}

/// Helper to extract the first available `<label>` value from a `g_labels` group.
fn extract_label(labels: &Option<Glabels>) -> Option<String> {
    labels.as_ref().and_then(|glabels| {
        glabels.items.iter().find_map(|item| {
            if let LabelChoice::Label(label) = item {
                Some(label.value.clone())
            } else {
                None
            }
        })
    })
}

/// Helper to extract the first available `<description>` value from a `g_labels` group.
fn extract_description(labels: &Option<Glabels>) -> Option<String> {
    labels.as_ref().and_then(|glabels| {
        glabels.items.iter().find_map(|item| {
            if let LabelChoice::Description(desc) = item {
                Some(desc.value.clone())
            } else {
                None
            }
        })
    })
}

/// Parses a `model::NetworkManagement` into a `types::NetworkManagement`.
pub(super) fn resolve_network_management(
    model: &model::net_mgmt::NetworkManagement,
) -> Result<types::NetworkManagement, XdcError> {
    // --- General Features ---
    let general_features = types::GeneralFeatures {
        dll_feature_mn: model.general_features.dll_feature_mn,
        nmt_boot_time_not_active: parse_u32_attr(Some(model.general_features.nmt_boot_time_not_active.clone())),
        nmt_cycle_time_max: parse_u32_attr(Some(model.general_features.nmt_cycle_time_max.clone())),
        nmt_cycle_time_min: parse_u32_attr(Some(model.general_features.nmt_cycle_time_min.clone())),
        nmt_error_entries: parse_u32_attr(Some(model.general_features.nmt_error_entries.clone())),
        nmt_max_cn_number: model.general_features.nmt_max_cn_number.clone().and_then(|s| s.parse().ok()),
        pdo_dynamic_mapping: model.general_features.pdo_dynamic_mapping,
        sdo_client: model.general_features.sdo_client,
        sdo_server: model.general_features.sdo_server,
        sdo_support_asnd: model.general_features.sdo_support_asnd,
        sdo_support_udp_ip: model.general_features.sdo_support_udp_ip,
    };

    // --- MN Features ---
    let mn_features = model.mn_features.as_ref().map(|mn| types::MnFeatures {
        dll_mn_feature_multiplex: mn.dll_mn_feature_multiplex,
        dll_mn_pres_chaining: mn.dll_mn_pres_chaining,
        nmt_simple_boot: mn.nmt_simple_boot,
    });

    // --- CN Features ---
    let cn_features = model.cn_features.as_ref().map(|cn| types::CnFeatures {
        dll_cn_feature_multiplex: cn.dll_cn_feature_multiplex,
        dll_cn_pres_chaining: cn.dll_cn_pres_chaining,
        nmt_cn_pre_op2_to_ready2_op: cn.nmt_cn_pre_op2_to_ready2_op.clone().and_then(|s| s.parse().ok()),
        nmt_cn_soc_2_preq: parse_u32_attr(Some(cn.nmt_cn_soc_2_preq.clone())),
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
    // --- Error List ---
    let errors = model
        .error_list
        .as_ref()
        .map_or(Vec::new(), |list| {
            list.error
                .iter()
                .map(|e| types::ErrorDefinition {
                    name: e.name.clone(),
                    value: e.value.clone(),
                    add_info: e.add_info.iter().map(|ai| types::AddInfo {
                        name: ai.name.clone(),
                        bit_offset: ai.bit_offset.parse().unwrap_or(0),
                        len: ai.len.parse().unwrap_or(0),
                        description: extract_description(&ai.labels),
                    }).collect(),
                })
                .collect()
        });
    
    // --- Static Error Bit Field ---
    let static_error_bit_field = model.static_error_bit_field.as_ref().map(|field| {
        field.error_bit.iter().map(|bit| {
            types::StaticErrorBit {
                name: bit.name.clone(),
                offset: bit.offset.parse().unwrap_or(0),
                label: extract_label(&bit.labels),
                description: extract_description(&bit.labels),
            }
        }).collect()
    });

    Ok(types::Diagnostic { errors, static_error_bit_field })
}