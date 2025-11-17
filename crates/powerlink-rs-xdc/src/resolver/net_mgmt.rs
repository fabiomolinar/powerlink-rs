// crates/powerlink-rs-xdc/src/resolver/net_mgmt.rs

use crate::error::XdcError;
use crate::model;
use crate::types;
use alloc::string::String;
use alloc::vec::Vec;
use crate::model::common::Glabels; // Import label helpers
use crate::resolver::utils; // Import the utils module

/// Helper to parse a string attribute as a u32.
fn parse_u32_attr(s: Option<String>) -> u32 {
    s.and_then(|val| val.parse().ok()).unwrap_or(0)
}


// REMOVED: `extract_label` - Now in `utils.rs`
// REMOVED: `extract_description` - Now in `utils.rs`

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
        nmt_cn_dna: cn.nmt_cn_dna.map(|dna_model| match dna_model {
            model::net_mgmt::CnFeaturesNmtCnDna::DoNotClear => types::NmtCnDna::DoNotClear,
            model::net_mgmt::CnFeaturesNmtCnDna::ClearOnPreOp1ToPreOp2 => types::NmtCnDna::ClearOnPreOp1ToPreOp2,
            model::net_mgmt::CnFeaturesNmtCnDna::ClearOnNmtResetNode => types::NmtCnDna::ClearOnNmtResetNode,
        }),
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
                    name: e.name.clone(), // Use unwrap_or_default for robustness
                    value: e.value.clone(), // Use unwrap_or_default for robustness
                    add_info: e.add_info.iter().map(|ai| types::AddInfo {
                        name: ai.name.clone(),
                        bit_offset: ai.bit_offset.parse().unwrap_or(0),
                        len: ai.len.parse().unwrap_or(0),
                        description: ai.labels.as_ref().and_then(utils::extract_description), // Use utils::
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
                label: bit.labels.as_ref().and_then(utils::extract_label), // Use utils::
                description: bit.labels.as_ref().and_then(utils::extract_description), // Use utils::
            }
        }).collect()
    });

    Ok(types::Diagnostic { errors, static_error_bit_field })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::common::{Description, Glabels, Label, LabelChoice};
    use crate::model::net_mgmt::{
        AddInfo, AddInfoValue, CnFeatures, CnFeaturesNmtCnDna, Diagnostic, Error, ErrorBit,
        ErrorList, GeneralFeatures, MnFeatures, NetworkManagement, StaticErrorBitField,
    };
    use crate::resolver::utils::{extract_description, extract_label};
    use crate::types;
    use alloc::string::ToString;
    use alloc::vec;

    /// Helper to parse a string attribute as a u8.
    fn parse_u8_attr(s: Option<String>) -> u8 {
        s.and_then(|val| val.parse().ok()).unwrap_or(0)
    }

    // --- Helper Function Tests ---

    #[test]
    fn test_parse_u32_attr() {
        assert_eq!(parse_u32_attr(Some("12345".to_string())), 12345);
        assert_eq!(parse_u32_attr(Some("0".to_string())), 0);
        assert_eq!(parse_u32_attr(None), 0);
        assert_eq!(parse_u32_attr(Some("invalid".to_string())), 0);
        assert_eq!(parse_u32_attr(Some("4294967295".to_string())), 4294967295);
    }

    #[test]
    fn test_parse_u8_attr() {
        assert_eq!(parse_u8_attr(Some("255".to_string())), 255);
        assert_eq!(parse_u8_attr(Some("0".to_string())), 0);
        assert_eq!(parse_u8_attr(None), 0);
        assert_eq!(parse_u8_attr(Some("invalid".to_string())), 0);
        assert_eq!(parse_u8_attr(Some("256".to_string())), 0); // Fails to parse, returns 0
    }

    #[test]
    fn test_extract_label() {

        // Test Some(Glabels) with no items
        let glabels_empty = Some(Glabels { items: vec![] });
        assert_eq!(extract_label(&glabels_empty.unwrap_or_default()), None);

        // Test Some(Glabels) with only description
        let glabels_desc = Some(Glabels {
            items: vec![LabelChoice::Description(Description {
                lang: "en".to_string(),
                value: "A description".to_string(),
                ..Default::default()
            })],
        });
        assert_eq!(extract_label(&glabels_desc.unwrap_or_default()), None);

        // Test Some(Glabels) with one label
        let glabels_one = Some(Glabels {
            items: vec![LabelChoice::Label(Label {
                lang: "en".to_string(),
                value: "First Label".to_string(),
            })],
        });
        assert_eq!(
            extract_label(&glabels_one.unwrap_or_default()),
            Some("First Label".to_string())
        );

        // Test Some(Glabels) with multiple items (should pick first label)
        let glabels_multi = Some(Glabels {
            items: vec![
                LabelChoice::Description(Description {
                    lang: "en".to_string(),
                    value: "A description".to_string(),
                    ..Default::default()
                }),
                LabelChoice::Label(Label {
                    lang: "en".to_string(),
                    value: "First Label".to_string(),
                }),
                LabelChoice::Label(Label {
                    lang: "de".to_string(),
                    value: "Zweite Beschriftung".to_string(),
                }),
            ],
        });
        assert_eq!(
            extract_label(&glabels_multi.unwrap_or_default()),
            Some("First Label".to_string())
        );
    }

    #[test]
    fn test_extract_description() {

        // Test Some(Glabels) with no items
        let glabels_empty = Some(Glabels { items: vec![] });
        assert_eq!(extract_description(&glabels_empty.unwrap_or_default()), None);

        // Test Some(Glabels) with only label
        let glabels_label = Some(Glabels {
            items: vec![LabelChoice::Label(Label {
                lang: "en".to_string(),
                value: "A label".to_string(),
            })],
        });
        assert_eq!(extract_description(&glabels_label.unwrap_or_default()), None);

        // Test Some(Glabels) with one description
        let glabels_one = Some(Glabels {
            items: vec![LabelChoice::Description(Description {
                lang: "en".to_string(),
                value: "First Description".to_string(),
                ..Default::default()
            })],
        });
        assert_eq!(
            extract_description(&glabels_one.unwrap_or_default()),
            Some("First Description".to_string())
        );

        // Test Some(Glabels) with multiple items (should pick first description)
        let glabels_multi = Some(Glabels {
            items: vec![
                LabelChoice::Label(Label {
                    lang: "en".to_string(),
                    value: "A label".to_string(),
                }),
                LabelChoice::Description(Description {
                    lang: "en".to_string(),
                    value: "First Description".to_string(),
                    ..Default::default()
                }),
                LabelChoice::Description(Description {
                    lang: "de".to_string(),
                    value: "Zweite Beschreibung".to_string(),
                    ..Default::default()
                }),
            ],
        });
        assert_eq!(
            extract_description(&glabels_multi.unwrap_or_default()),
            Some("First Description".to_string())
        );
    }

    // --- Main Function Tests ---

    #[test]
    fn test_resolve_diagnostic() {
        let model_diag = Diagnostic {
            error_list: Some(ErrorList {
                error: vec![Error {
                    name: "TestError".to_string(),
                    value: "0x8001".to_string(),
                    labels: Some(Glabels {
                        items: vec![LabelChoice::Description(Description {
                            lang: "en".to_string(),
                            value: "A test error".to_string(),
                            ..Default::default()
                        })],
                    }),
                    add_info: vec![AddInfo {
                        name: "SubCode".to_string(),
                        bit_offset: "0".to_string(),
                        len: "8".to_string(),
                        labels: Some(Glabels {
                            items: vec![LabelChoice::Description(Description {
                                lang: "en".to_string(),
                                value: "Sub error code".to_string(),
                                ..Default::default()
                            })],
                        }),
                        value: vec![AddInfoValue {
                            name: "Code1".to_string(),
                            value: "1".to_string(),
                            labels: None,
                        }],
                    }],
                }],
            }),
            static_error_bit_field: Some(StaticErrorBitField {
                error_bit: vec![ErrorBit {
                    name: "CommError".to_string(),
                    offset: "0".to_string(),
                    labels: Some(Glabels {
                        items: vec![LabelChoice::Label(Label {
                            lang: "en".to_string(),
                            value: "Communication Error".to_string(),
                        })],
                    }),
                }],
            }),
        };

        let public_diag = resolve_diagnostic(&model_diag).unwrap();

        // Check ErrorList
        assert_eq!(public_diag.errors.len(), 1);
        assert_eq!(public_diag.errors[0].name, "TestError");
        assert_eq!(public_diag.errors[0].value, "0x8001");

        // Check AddInfo
        assert_eq!(public_diag.errors[0].add_info.len(), 1);
        assert_eq!(public_diag.errors[0].add_info[0].name, "SubCode");
        assert_eq!(public_diag.errors[0].add_info[0].bit_offset, 0);
        assert_eq!(public_diag.errors[0].add_info[0].len, 8);
        assert_eq!(
            public_diag.errors[0].add_info[0].description,
            Some("Sub error code".to_string())
        );

        // Check StaticErrorBitField
        let static_bits = public_diag.static_error_bit_field.unwrap();
        assert_eq!(static_bits.len(), 1);
        assert_eq!(static_bits[0].name, "CommError");
        assert_eq!(static_bits[0].offset, 0);
        assert_eq!(
            static_bits[0].label,
            Some("Communication Error".to_string())
        );
        assert_eq!(static_bits[0].description, None);
    }

    #[test]
    fn test_resolve_diagnostic_empty() {
        let model_diag = Diagnostic {
            error_list: None,
            static_error_bit_field: None,
        };
        let public_diag = resolve_diagnostic(&model_diag).unwrap();
        assert!(public_diag.errors.is_empty());
        assert!(public_diag.static_error_bit_field.is_none());

        let model_diag_empty_lists = Diagnostic {
            error_list: Some(ErrorList::default()),
            static_error_bit_field: Some(StaticErrorBitField::default()),
        };
        let public_diag_empty = resolve_diagnostic(&model_diag_empty_lists).unwrap();
        assert!(public_diag_empty.errors.is_empty());
        assert!(public_diag_empty.static_error_bit_field.unwrap().is_empty());
    }

    #[test]
    fn test_resolve_network_management() {
        let model_nm = NetworkManagement {
            general_features: GeneralFeatures {
                dll_feature_mn: true,
                nmt_boot_time_not_active: "1000".to_string(),
                nmt_cycle_time_max: "5000".to_string(),
                nmt_cycle_time_min: "1000".to_string(),
                nmt_error_entries: "10".to_string(),
                nmt_max_cn_number: Some("5".to_string()),
                pdo_dynamic_mapping: Some(true),
                sdo_server: Some(false),
                ..Default::default()
            },
            mn_features: Some(MnFeatures {
                dll_mn_feature_multiplex: Some(true),
                dll_mn_pres_chaining: Some(false),
                nmt_simple_boot: true,
                ..Default::default()
            }),
            cn_features: Some(CnFeatures {
                dll_cn_feature_multiplex: Some(true),
                nmt_cn_soc_2_preq: "50".to_string(),
                nmt_cn_dna: Some(CnFeaturesNmtCnDna::ClearOnPreOp1ToPreOp2),
                ..Default::default()
            }),
            diagnostic: None,
            device_commissioning: None,
        };

        let public_nm = resolve_network_management(&model_nm).unwrap();

        // Check GeneralFeatures
        assert_eq!(public_nm.general_features.dll_feature_mn, true);
        assert_eq!(public_nm.general_features.nmt_boot_time_not_active, 1000);
        assert_eq!(public_nm.general_features.nmt_cycle_time_max, 5000);
        assert_eq!(public_nm.general_features.nmt_cycle_time_min, 1000);
        assert_eq!(public_nm.general_features.nmt_error_entries, 10);
        assert_eq!(public_nm.general_features.nmt_max_cn_number, Some(5));
        assert_eq!(public_nm.general_features.pdo_dynamic_mapping, Some(true));
        assert_eq!(public_nm.general_features.sdo_server, Some(false));

        // Check MNFeatures
        let mnf = public_nm.mn_features.unwrap();
        assert_eq!(mnf.dll_mn_feature_multiplex, Some(true));
        assert_eq!(mnf.dll_mn_pres_chaining, Some(false));
        assert_eq!(mnf.nmt_simple_boot, true);

        // Check CNFeatures
        let cnf = public_nm.cn_features.unwrap();
        assert_eq!(cnf.dll_cn_feature_multiplex, Some(true));
        assert_eq!(cnf.nmt_cn_soc_2_preq, 50);
        assert_eq!(
            cnf.nmt_cn_dna,
            Some(types::NmtCnDna::ClearOnPreOp1ToPreOp2)
        );
    }
}