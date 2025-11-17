// crates/powerlink-rs-xdc/src/resolver/device_function.rs

//! Handles resolving the `<DeviceFunction>` block from the model to public types.

use crate::error::XdcError;
use crate::model;
use crate::resolver::utils; // Import the utils module
use crate::types;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// --- Label Helpers ---

// REMOVED: `extract_label` - Now in `utils.rs`
// REMOVED: `extract_description` - Now in `utils.rs`

// --- Sub-Resolvers ---

/// Resolves `<capabilities>`.
fn resolve_capabilities(
    model: &model::device_function::Capabilities,
) -> Result<types::Capabilities, XdcError> {
    let characteristics = model
        .characteristics_list
        .iter()
        .map(|cl| {
            Ok::<_, XdcError>(types::CharacteristicList {
                category: cl.category.as_ref().and_then(|c| utils::extract_label(&c.labels.items)),
                characteristics: cl
                    .characteristic
                    .iter()
                    .map(|c| {
                        Ok::<_, XdcError>(types::Characteristic {
                            name: utils::extract_label(&c.characteristic_name.items) // FIX: Pass .items
                                .ok_or(XdcError::MissingElement {
                                    element: "characteristicName/label",
                                })?,
                            content: c
                                .characteristic_content
                                .iter()
                                .map(|cc| {
                                    utils::extract_label(&cc.items).ok_or(XdcError::MissingElement { // FIX: Pass .items
                                        element: "characteristicContent/label",
                                    })
                                })
                                .collect::<Result<Vec<_>, _>>()?,
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let standard_compliance = model
        .standard_compliance_list
        .as_ref()
        .map_or(Vec::new(), |scl| {
            scl.compliant_with
                .iter()
                .map(|cw| types::StandardCompliance {
                    name: cw.name.clone(),
                    range: cw.range.clone().unwrap_or("international".to_string()),
                    description: utils::extract_description(&cw.labels.items), // FIX: Pass .items
                })
                .collect()
        });

    Ok(types::Capabilities {
        characteristics,
        standard_compliance,
    })
}

/// Resolves `<picturesList>`.
fn resolve_pictures_list(
    model: &model::device_function::PicturesList,
) -> Result<Vec<types::Picture>, XdcError> {
    model
        .picture
        .iter()
        .map(|p| {
            Ok(types::Picture {
                uri: p.uri.clone(),
                picture_type: p.picture_type.clone().unwrap_or("none".to_string()),
                number: p
                    .number
                    .as_ref()
                    .and_then(|n| n.parse::<u32>().ok()),
                label: utils::extract_label(&p.labels.items), // FIX: Pass .items
                description: utils::extract_description(&p.labels.items), // FIX: Pass .items
            })
        })
        .collect()
}

/// Resolves `<dictionaryList>`.
fn resolve_dictionary_list(
    model: &model::device_function::DictionaryList,
) -> Result<Vec<types::Dictionary>, XdcError> {
    Ok(model
        .dictionary
        .iter()
        .map(|d| types::Dictionary {
            uri: d.file.uri.clone(),
            lang: d.lang.clone(),
            dict_id: d.dict_id.clone(),
        })
        .collect())
}

/// Resolves `<connectorList>`.
fn resolve_connector_list(
    model: &model::device_function::ConnectorList,
) -> Result<Vec<types::Connector>, XdcError> {
    Ok(model
        .connector
        .iter()
        .map(|c| types::Connector {
            id: c.id.clone(),
            connector_type: c
                .connector_type
                .clone()
                .unwrap_or("POWERLINK".to_string()),
            interface_id_ref: c.interface_id_ref.clone(),
            label: utils::extract_label(&c.labels.items), // FIX: Pass .items
            description: utils::extract_description(&c.labels.items), // FIX: Pass .items
        })
        .collect())
}

/// Resolves `<firmwareList>`.
fn resolve_firmware_list(
    model: &model::device_function::FirmwareList,
) -> Result<Vec<types::Firmware>, XdcError> {
    model
        .firmware
        .iter()
        .map(|f| {
            Ok(types::Firmware {
                uri: f.uri.clone(),
                device_revision_number: f.device_revision_number.parse::<u32>().map_err(
                    |_| XdcError::InvalidAttributeFormat {
                        attribute: "deviceRevisionNumber",
                    },
                )?,
                build_date: f.build_date.clone(),
                label: utils::extract_label(&f.labels.items), // FIX: Pass .items
                description: utils::extract_description(&f.labels.items), // FIX: Pass .items
            })
        })
        .collect()
}

/// Resolves `<classificationList>`.
fn resolve_classification_list(
    model: &model::device_function::ClassificationList,
) -> Result<Vec<types::Classification>, XdcError> {
    Ok(model
        .classification
        .iter()
        .map(|c| types::Classification {
            value: c.value.clone(),
        })
        .collect())
}

// --- Main Resolver ---

/// Parses a `model::DeviceFunction` into a `types::DeviceFunction`.
pub(super) fn resolve_device_function(
    model_vec: &[model::device_function::DeviceFunction],
) -> Result<Vec<types::DeviceFunction>, XdcError> {
    model_vec
        .iter()
        .map(|model| {
            let capabilities = model
                .capabilities
                .as_ref()
                .map(resolve_capabilities)
                .transpose()?;
            
            let pictures = model
                .pictures_list
                .as_ref()
                .map(resolve_pictures_list)
                .transpose()?
                .unwrap_or_default();
            
            let dictionaries = model
                .dictionary_list
                .as_ref()
                .map(resolve_dictionary_list)
                .transpose()?
                .unwrap_or_default();
            
            let connectors = model
                .connector_list
                .as_ref()
                .map(resolve_connector_list)
                .transpose()?
                .unwrap_or_default();
            
            let firmware_list = model
                .firmware_list
                .as_ref()
                .map(resolve_firmware_list)
                .transpose()?
                .unwrap_or_default();
            
            let classifications = model
                .classification_list
                .as_ref()
                .map(resolve_classification_list)
                .transpose()?
                .unwrap_or_default();

            Ok(types::DeviceFunction {
                capabilities,
                pictures,
                dictionaries,
                connectors,
                firmware_list,
                classifications,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::common::{Glabels, Label, LabelChoice, Description};
    use crate::model::device_function as model_df;
    use crate::types;
    use alloc::string::ToString;
    use alloc::vec;

    // --- MOCK DATA FACTORIES ---

    fn create_test_label(text: &str) -> LabelChoice {
        LabelChoice::Label(Label {
            lang: "en".to_string(),
            value: text.to_string(),
        })
    }

    fn create_test_desc(text: &str) -> LabelChoice {
        LabelChoice::Description(Description {
            lang: "en".to_string(),
            value: text.to_string(),
            ..Default::default()
        })
    }

    // --- UNIT TESTS ---

    #[test]
    fn test_resolve_capabilities() {
        let model_caps = model_df::Capabilities {
            characteristics_list: vec![model_df::CharacteristicsList {
                category: Some(model_df::Category {
                    labels: Glabels {
                        items: vec![create_test_label("General")],
                    },
                }),
                characteristic: vec![model_df::Characteristic {
                    characteristic_name: model_df::CharacteristicName {
                        items: vec![create_test_label("Rate")], // FIX: Use items
                    },
                    characteristic_content: vec![
                        model_df::CharacteristicContent {
                            items: vec![create_test_label("100 MBit/s")], // FIX: Use items
                            ..Default::default()
                        },
                        model_df::CharacteristicContent {
                            items: vec![create_test_label("Full Duplex")], // FIX: Use items
                            ..Default::default()
                        },
                    ],
                }],
            }],
            standard_compliance_list: Some(model_df::StandardComplianceList {
                compliant_with: vec![model_df::CompliantWith {
                    labels: Glabels {
                        items: vec![create_test_desc("Some standard desc")],
                    },
                    name: "EN 12345".to_string(),
                    range: Some("international".to_string()),
                }],
            }),
        };

        let pub_caps = resolve_capabilities(&model_caps).unwrap();

        // Check CharacteristicsList
        assert_eq!(pub_caps.characteristics.len(), 1);
        let char_list = &pub_caps.characteristics[0];
        assert_eq!(char_list.category, Some("General".to_string()));

        // Check Characteristic
        assert_eq!(char_list.characteristics.len(), 1);
        let char = &char_list.characteristics[0];
        assert_eq!(char.name, "Rate");
        assert_eq!(char.content.len(), 2);
        assert_eq!(char.content[0], "100 MBit/s");
        assert_eq!(char.content[1], "Full Duplex");

        // Check StandardCompliance
        assert_eq!(pub_caps.standard_compliance.len(), 1);
        let std = &pub_caps.standard_compliance[0];
        assert_eq!(std.name, "EN 12345");
        assert_eq!(std.range, "international");
        assert_eq!(std.description, Some("Some standard desc".to_string()));
    }

    #[test]
    fn test_resolve_pictures_list() {
        let model_pics = model_df::PicturesList {
            picture: vec![model_df::Picture {
                labels: Glabels {
                    items: vec![
                        create_test_label("My Device Icon"),
                        create_test_desc("Icon for topology"),
                    ],
                },
                uri: "./icon.png".to_string(),
                picture_type: Some("icon".to_string()),
                number: Some("1".to_string()),
            }],
        };

        let pub_pics = resolve_pictures_list(&model_pics).unwrap();

        assert_eq!(pub_pics.len(), 1);
        let pic = &pub_pics[0];
        assert_eq!(pic.uri, "./icon.png");
        assert_eq!(pic.picture_type, "icon");
        assert_eq!(pic.number, Some(1));
        assert_eq!(pic.label, Some("My Device Icon".to_string()));
        assert_eq!(pic.description, Some("Icon for topology".to_string()));
    }

    #[test]
    fn test_resolve_dictionary_list() {
        let model_dict_list = model_df::DictionaryList {
            dictionary: vec![model_df::Dictionary {
                file: model_df::DictionaryFile {
                    uri: "./texts_en.xml".to_string(),
                },
                lang: "en".to_string(),
                dict_id: "texts_en".to_string(),
            }],
        };

        let pub_dict_list = resolve_dictionary_list(&model_dict_list).unwrap();
        assert_eq!(pub_dict_list.len(), 1);
        let dict = &pub_dict_list[0];
        assert_eq!(dict.uri, "./texts_en.xml");
        assert_eq!(dict.lang, "en");
        assert_eq!(dict.dict_id, "texts_en");
    }

    #[test]
    fn test_resolve_connector_list() {
        let model_conn_list = model_df::ConnectorList {
            connector: vec![model_df::Connector {
                labels: Glabels {
                    items: vec![create_test_label("EPL 1")],
                },
                id: "conn1".to_string(),
                connector_type: Some("RJ45".to_string()),
                interface_id_ref: Some("if_1".to_string()),
                ..Default::default()
            }],
        };

        let pub_conn_list = resolve_connector_list(&model_conn_list).unwrap();
        assert_eq!(pub_conn_list.len(), 1);
        let conn = &pub_conn_list[0];
        assert_eq!(conn.id, "conn1");
        assert_eq!(conn.connector_type, "RJ45");
        assert_eq!(conn.interface_id_ref, Some("if_1".to_string()));
        assert_eq!(conn.label, Some("EPL 1".to_string()));
    }

    #[test]
    fn test_resolve_firmware_list() {
        let model_fw_list = model_df::FirmwareList {
            firmware: vec![model_df::Firmware {
                labels: Glabels {
                    items: vec![create_test_label("Main Firmware")],
                },
                uri: "./fw.bin".to_string(),
                device_revision_number: "123".to_string(),
                build_date: Some("2024-01-01T12:00:00".to_string()),
            }],
        };

        let pub_fw_list = resolve_firmware_list(&model_fw_list).unwrap();
        assert_eq!(pub_fw_list.len(), 1);
        let fw = &pub_fw_list[0];
        assert_eq!(fw.uri, "./fw.bin");
        assert_eq!(fw.device_revision_number, 123);
        assert_eq!(fw.build_date, Some("2024-01-01T12:00:00".to_string()));
        assert_eq!(fw.label, Some("Main Firmware".to_string()));
    }

    #[test]
    fn test_resolve_classification_list() {
        let model_class_list = model_df::ClassificationList {
            classification: vec![
                model_df::Classification {
                    value: "IO".to_string(),
                },
                model_df::Classification {
                    value: "Digital".to_string(),
                },
            ],
        };

        let pub_class_list = resolve_classification_list(&model_class_list).unwrap();
        assert_eq!(pub_class_list.len(), 2);
        assert_eq!(pub_class_list[0].value, "IO");
        assert_eq!(pub_class_list[1].value, "Digital");
    }

    #[test]
    fn test_resolve_device_function_full() {
        let model_df_vec = vec![model_df::DeviceFunction {
            capabilities: Some(model_df::Capabilities { ..Default::default() }),
            pictures_list: Some(model_df::PicturesList {
                picture: vec![model_df::Picture {
                    uri: "uri1".to_string(),
                    ..Default::default()
                }],
            }),
            dictionary_list: Some(model_df::DictionaryList {
                dictionary: vec![model_df::Dictionary {
                    dict_id: "dict1".to_string(),
                    lang: "en".to_string(),
                    file: model_df::DictionaryFile { uri: "uri2".to_string() }
                }],
            }),
            connector_list: Some(model_df::ConnectorList {
                connector: vec![model_df::Connector {
                    id: "conn1".to_string(),
                    ..Default::default()
                }],
            }),
            firmware_list: Some(model_df::FirmwareList {
                firmware: vec![model_df::Firmware {
                    uri: "uri3".to_string(),
                    device_revision_number: "1".to_string(),
                    ..Default::default()
                }],
            }),
            classification_list: Some(model_df::ClassificationList {
                classification: vec![model_df::Classification {
                    value: "IO".to_string(),
                }],
            }),
        }];

        let pub_df_vec = resolve_device_function(&model_df_vec).unwrap();

        assert_eq!(pub_df_vec.len(), 1);
        let pub_df = &pub_df_vec[0];

        assert!(pub_df.capabilities.is_some());
        assert_eq!(pub_df.pictures.len(), 1);
        assert_eq!(pub_df.pictures[0].uri, "uri1");
        assert_eq!(pub_df.dictionaries.len(), 1);
        assert_eq!(pub_df.dictionaries[0].dict_id, "dict1");
        assert_eq!(pub_df.connectors.len(), 1);
        assert_eq!(pub_df.connectors[0].id, "conn1");
        assert_eq!(pub_df.firmware_list.len(), 1);
        assert_eq!(pub_df.firmware_list[0].device_revision_number, 1);
        assert_eq!(pub_df.classifications.len(), 1);
        assert_eq!(pub_df.classifications[0].value, "IO");
    }
}