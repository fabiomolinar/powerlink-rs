// crates/powerlink-rs-xdc/src/builder/device_function.rs

//! Contains builder functions to convert `types::DeviceFunction` into `model::DeviceFunction`.

#![allow(clippy::pedantic)] // XML schema names are not idiomatic Rust

use crate::{model, types};
use alloc::{string::{String, ToString}, vec, vec::Vec};
use crate::model::common::{Glabels, Label, LabelChoice, Description};

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
        category: public.category.as_ref().map(|c| model::device_function::Category {
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
fn build_model_capabilities(
    public: &types::Capabilities,
) -> model::device_function::Capabilities {
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
                dictionary: public.dictionaries.iter().map(build_model_dictionary).collect(),
            }),
            connector_list: Some(model::device_function::ConnectorList {
                connector: public.connectors.iter().map(build_model_connector).collect(),
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