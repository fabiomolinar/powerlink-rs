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
                category: cl.category.as_ref().and_then(|c| utils::extract_label(&c.labels)),
                characteristics: cl
                    .characteristic
                    .iter()
                    .map(|c| {
                        Ok::<_, XdcError>(types::Characteristic {
                            name: utils::extract_label(&c.characteristic_name.labels)
                                .ok_or(XdcError::MissingElement {
                                    element: "characteristicName/label",
                                })?,
                            content: c
                                .characteristic_content
                                .iter()
                                .map(|cc| {
                                    utils::extract_label(&cc.labels).ok_or(XdcError::MissingElement {
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
                    description: utils::extract_description(&cw.labels), // Use utils::
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
                label: utils::extract_label(&p.labels), // Use utils::
                description: utils::extract_description(&p.labels), // Use utils::
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
            label: utils::extract_label(&c.labels), // Use utils::
            description: utils::extract_description(&c.labels), // Use utils::
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
                label: utils::extract_label(&f.labels), // Use utils::
                description: utils::extract_description(&f.labels), // Use utils::
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