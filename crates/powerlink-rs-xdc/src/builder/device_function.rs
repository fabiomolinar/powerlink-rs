// crates/powerlink-rs-xdc/src/builder/device_function.rs

//! Contains builder functions to convert `types::DeviceFunction` into `model::DeviceFunction`.

#![allow(clippy::pedantic)] // XML schema names are not idiomatic Rust

use crate::model::common::{Description, Glabels, Label, LabelChoice};
use crate::{model, types};
use alloc::{
    string::{String, ToString},
    vec::Vec,
};

/// Helper to create a `Glabels` struct from optional label and description strings.
fn build_glabels(label: Option<&String>, description: Option<&String>) -> Glabels {
    let mut items = Vec::new();
    if let Some(l) = label {
        items.push(LabelChoice::Label(Label {
            lang: "en".to_string(), // Default to "en" for serialization
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
    Glabels { items }
}

/// Converts a public `types::Characteristic` into a `model::device_function::Characteristic`.
fn build_model_characteristic(
    public: &types::Characteristic,
) -> model::device_function::Characteristic {
    model::device_function::Characteristic {
        characteristic_name: model::device_function::CharacteristicName {
            labels: build_glabels(Some(&public.name), None),
        },
        characteristic_content: public
            .content
            .iter()
            .map(|c| model::device_function::CharacteristicContent {
                labels: build_glabels(Some(c), None),
            })
            .collect(),
    }
}

/// Converts a public `types::CharacteristicList` into a `model::device_function::CharacteristicsList`.
fn build_model_characteristics_list(
    public: &types::CharacteristicList,
) -> model::device_function::CharacteristicsList {
    model::device_function::CharacteristicsList {
        category: public
            .category
            .as_ref()
            .map(|c| model::device_function::Category {
                labels: build_glabels(Some(c), None),
            }),
        characteristic: public
            .characteristics
            .iter()
            .map(build_model_characteristic)
            .collect(),
    }
}

/// Converts a public `types::StandardCompliance` into a `model::device_function::CompliantWith`.
fn build_model_compliant_with(
    public: &types::StandardCompliance,
) -> model::device_function::CompliantWith {
    model::device_function::CompliantWith {
        labels: build_glabels(None, public.description.as_ref()),
        name: public.name.clone(),
        range: Some(public.range.clone()),
    }
}

/// Converts a public `types::Capabilities` into a `model::device_function::Capabilities`.
fn build_model_capabilities(public: &types::Capabilities) -> model::device_function::Capabilities {
    model::device_function::Capabilities {
        characteristics_list: public
            .characteristics
            .iter()
            .map(build_model_characteristics_list)
            .collect(),
        standard_compliance_list: Some(model::device_function::StandardComplianceList {
            compliant_with: public
                .standard_compliance
                .iter()
                .map(build_model_compliant_with)
                .collect(),
        }),
    }
}

/// Converts a public `types::Picture` into a `model::device_function::Picture`.
fn build_model_picture(public: &types::Picture) -> model::device_function::Picture {
    model::device_function::Picture {
        labels: build_glabels(public.label.as_ref(), public.description.as_ref()),
        uri: public.uri.clone(),
        picture_type: Some(public.picture_type.clone()),
        number: public.number.map(|n| n.to_string()),
    }
}

/// Converts a public `types::Dictionary` into a `model::device_function::Dictionary`.
fn build_model_dictionary(public: &types::Dictionary) -> model::device_function::Dictionary {
    model::device_function::Dictionary {
        file: model::device_function::DictionaryFile {
            uri: public.uri.clone(),
        },
        lang: public.lang.clone(),
        dict_id: public.dict_id.clone(),
    }
}

/// Converts a public `types::Connector` into a `model::device_function::Connector`.
fn build_model_connector(public: &types::Connector) -> model::device_function::Connector {
    model::device_function::Connector {
        labels: build_glabels(public.label.as_ref(), public.description.as_ref()),
        id: public.id.clone(),
        pos_x: None, // posX/posY are not in public types
        pos_y: None,
        connector_type: Some(public.connector_type.clone()),
        interface_id_ref: public.interface_id_ref.clone(),
        positioning: None, // positioning is not in public types
    }
}

/// Converts a public `types::Firmware` into a `model::device_function::Firmware`.
fn build_model_firmware(public: &types::Firmware) -> model::device_function::Firmware {
    model::device_function::Firmware {
        labels: build_glabels(public.label.as_ref(), public.description.as_ref()),
        uri: public.uri.clone(),
        device_revision_number: public.device_revision_number.to_string(),
        build_date: public.build_date.clone(),
    }
}

/// Converts a public `types::Classification` into a `model::device_function::Classification`.
fn build_model_classification(
    public: &types::Classification,
) -> model::device_function::Classification {
    model::device_function::Classification {
        value: public.value.clone(),
    }
}

/// Converts a slice of public `types::DeviceFunction` into a `Vec<model::device_function::DeviceFunction>`.
pub(super) fn build_model_device_function(
    public_vec: &[types::DeviceFunction],
) -> Vec<model::device_function::DeviceFunction> {
    public_vec
        .iter()
        .map(|public| model::device_function::DeviceFunction {
            capabilities: public.capabilities.as_ref().map(build_model_capabilities),
            pictures_list: Some(model::device_function::PicturesList {
                picture: public.pictures.iter().map(build_model_picture).collect(),
            }),
            dictionary_list: Some(model::device_function::DictionaryList {
                dictionary: public
                    .dictionaries
                    .iter()
                    .map(build_model_dictionary)
                    .collect(),
            }),
            connector_list: Some(model::device_function::ConnectorList {
                connector: public
                    .connectors
                    .iter()
                    .map(build_model_connector)
                    .collect(),
            }),
            firmware_list: Some(model::device_function::FirmwareList {
                firmware: public
                    .firmware_list
                    .iter()
                    .map(build_model_firmware)
                    .collect(),
            }),
            classification_list: Some(model::device_function::ClassificationList {
                classification: public
                    .classifications
                    .iter()
                    .map(build_model_classification)
                    .collect(),
            }),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::common::LabelChoice;
    use crate::types;
    use alloc::string::ToString;
    use alloc::vec;

    // --- UNIT TESTS for helpers ---

    #[test]
    fn test_build_glabels() {
        // 1. Test with label and description
        let label = Some("My Label".to_string());
        let desc = Some("My Description".to_string());
        let glabels1 = build_glabels(label.as_ref(), desc.as_ref());

        assert_eq!(glabels1.items.len(), 2);
        assert!(matches!(&glabels1.items[0], LabelChoice::Label(l) if l.value == "My Label"));
        assert!(matches!(&glabels1.items[1], LabelChoice::Description(d) if d.value == "My Description"));

        // 2. Test with only label
        let glabels2 = build_glabels(label.as_ref(), None);
        assert_eq!(glabels2.items.len(), 1);
        assert!(matches!(&glabels2.items[0], LabelChoice::Label(l) if l.value == "My Label"));

        // 3. Test with only description
        let glabels3 = build_glabels(None, desc.as_ref());
        assert_eq!(glabels3.items.len(), 1);
        assert!(matches!(&glabels3.items[0], LabelChoice::Description(d) if d.value == "My Description"));

        // 4. Test with None
        let glabels4 = build_glabels(None, None);
        assert!(glabels4.items.is_empty());
    }

    #[test]
    fn test_build_model_characteristic() {
        let pub_char = types::Characteristic {
            name: "Rate".to_string(),
            content: vec!["100 MBit/s".to_string(), "Full Duplex".to_string()],
        };
        let model_char = build_model_characteristic(&pub_char);

        // Check name (which is a label)
        assert_eq!(model_char.characteristic_name.labels.items.len(), 1);
        assert!(
            matches!(&model_char.characteristic_name.labels.items[0], LabelChoice::Label(l) if l.value == "Rate")
        );

        // Check content (which are also labels)
        assert_eq!(model_char.characteristic_content.len(), 2);
        assert!(
            matches!(&model_char.characteristic_content[0].labels.items[0], LabelChoice::Label(l) if l.value == "100 MBit/s")
        );
        assert!(
            matches!(&model_char.characteristic_content[1].labels.items[0], LabelChoice::Label(l) if l.value == "Full Duplex")
        );
    }

    #[test]
    fn test_build_model_capabilities() {
        let pub_caps = types::Capabilities {
            characteristics: vec![types::CharacteristicList {
                category: Some("General".to_string()),
                characteristics: vec![types::Characteristic {
                    name: "Rate".to_string(),
                    ..Default::default()
                }],
            }],
            standard_compliance: vec![types::StandardCompliance {
                name: "EN 12345".to_string(),
                range: "international".to_string(),
                description: Some("Test standard".to_string()),
            }],
        };
        let model_caps = build_model_capabilities(&pub_caps);

        // Check characteristics list
        assert_eq!(model_caps.characteristics_list.len(), 1);
        let model_char_list = &model_caps.characteristics_list[0];
        assert!(model_char_list.category.is_some());
        let cat_label = &model_char_list.category.as_ref().unwrap().labels.items[0];
        assert!(matches!(cat_label, LabelChoice::Label(l) if l.value == "General"));
        assert_eq!(model_char_list.characteristic.len(), 1);

        // Check standard compliance
        let std_list = model_caps.standard_compliance_list.unwrap();
        assert_eq!(std_list.compliant_with.len(), 1);
        let model_std = &std_list.compliant_with[0];
        assert_eq!(model_std.name, "EN 12345");
        assert_eq!(model_std.range, Some("international".to_string()));
        let std_desc = &model_std.labels.items[0];
        assert!(matches!(std_desc, LabelChoice::Description(d) if d.value == "Test standard"));
    }

    #[test]
    fn test_build_model_picture() {
        let pub_pic = types::Picture {
            uri: "./icon.png".to_string(),
            picture_type: "icon".to_string(),
            number: Some(1),
            label: Some("Icon".to_string()),
            description: Some("A device icon".to_string()),
        };
        let model_pic = build_model_picture(&pub_pic);
        assert_eq!(model_pic.uri, "./icon.png");
        assert_eq!(model_pic.picture_type, Some("icon".to_string()));
        assert_eq!(model_pic.number, Some("1".to_string()));
        assert_eq!(model_pic.labels.items.len(), 2); // Label and Description
    }

    #[test]
    fn test_build_model_dictionary() {
        let pub_dict = types::Dictionary {
            uri: "./texts_en.xml".to_string(),
            lang: "en".to_string(),
            dict_id: "en_dict".to_string(),
        };
        let model_dict = build_model_dictionary(&pub_dict);
        assert_eq!(model_dict.file.uri, "./texts_en.xml");
        assert_eq!(model_dict.lang, "en");
        assert_eq!(model_dict.dict_id, "en_dict");
    }

    #[test]
    fn test_build_model_connector() {
        let pub_conn = types::Connector {
            id: "conn1".to_string(),
            connector_type: "RJ45".to_string(),
            interface_id_ref: Some("if_1".to_string()),
            label: Some("EPL 1".to_string()),
            ..Default::default()
        };
        let model_conn = build_model_connector(&pub_conn);
        assert_eq!(model_conn.id, "conn1");
        assert_eq!(model_conn.connector_type, Some("RJ45".to_string()));
        assert_eq!(model_conn.interface_id_ref, Some("if_1".to_string()));
        assert_eq!(model_conn.labels.items.len(), 1); // Only label
    }

    #[test]
    fn test_build_model_firmware() {
        let pub_fw = types::Firmware {
            uri: "./fw.bin".to_string(),
            device_revision_number: 123,
            build_date: Some("2024-01-01".to_string()),
            ..Default::default()
        };
        let model_fw = build_model_firmware(&pub_fw);
        assert_eq!(model_fw.uri, "./fw.bin");
        assert_eq!(model_fw.device_revision_number, "123");
        assert_eq!(model_fw.build_date, Some("2024-01-01".to_string()));
    }

    #[test]
    fn test_build_model_classification() {
        let pub_class = types::Classification {
            value: "IO".to_string(),
        };
        let model_class = build_model_classification(&pub_class);
        assert_eq!(model_class.value, "IO");
    }

    // --- INTEGRATION TEST for build_model_device_function ---

    #[test]
    fn test_build_model_device_function() {
        let pub_df_vec = vec![types::DeviceFunction {
            capabilities: Some(types::Capabilities {
                characteristics: vec![types::CharacteristicList {
                    category: Some("General".to_string()),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            pictures: vec![types::Picture {
                uri: "./icon.png".to_string(),
                ..Default::default()
            }],
            dictionaries: vec![types::Dictionary {
                dict_id: "en_dict".to_string(),
                ..Default::default()
            }],
            connectors: vec![types::Connector {
                id: "conn1".to_string(),
                ..Default::default()
            }],
            firmware_list: vec![types::Firmware {
                uri: "./fw.bin".to_string(),
                device_revision_number: 1,
                ..Default::default()
            }],
            classifications: vec![types::Classification {
                value: "IO".to_string(),
            }],
        }];

        let model_df_vec = build_model_device_function(&pub_df_vec);

        assert_eq!(model_df_vec.len(), 1);
        let model_df = &model_df_vec[0];

        // Check that all lists were populated
        assert!(model_df.capabilities.is_some());
        assert!(model_df.pictures_list.is_some());
        assert!(model_df.dictionary_list.is_some());
        assert!(model_df.connector_list.is_some());
        assert!(model_df.firmware_list.is_some());
        assert!(model_df.classification_list.is_some());

        // Spot check content
        assert_eq!(
            model_df.capabilities.as_ref().unwrap().characteristics_list[0]
                .category
                .as_ref()
                .unwrap()
                .labels
                .items
                .len(),
            1
        );
        assert_eq!(
            model_df.pictures_list.as_ref().unwrap().picture[0].uri,
            "./icon.png"
        );
        assert_eq!(
            model_df.dictionary_list.as_ref().unwrap().dictionary[0].dict_id,
            "en_dict"
        );
        assert_eq!(
            model_df.connector_list.as_ref().unwrap().connector[0].id,
            "conn1"
        );
        assert_eq!(
            model_df.firmware_list.as_ref().unwrap().firmware[0].device_revision_number,
            "1"
        );
        assert_eq!(
            model_df.classification_list.as_ref().unwrap().classification[0].value,
            "IO"
        );
    }
}