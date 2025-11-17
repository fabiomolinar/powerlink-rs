// crates/powerlink-rs-xdc/src/resolver/device_manager.rs

//! Handles resolving the `<DeviceManager>` block from the model to public types.

use crate::error::XdcError;
use crate::model;
use crate::resolver::modular; // Import the new modular resolver
use crate::types;
use alloc::string::{String, ToString}; // Fix: Add String import
use alloc::vec::Vec;

/// Helper to extract the first available `<label>` value from a `g_labels` group.
fn extract_label(labels: &model::common::Glabels) -> Option<String> {
    labels.items.iter().find_map(|item| {
        if let model::common::LabelChoice::Label(label) = item {
            Some(label.value.clone())
        } else {
            None
        }
    })
}

/// Helper to extract the first available `<description>` value from a `g_labels` group.
fn extract_description(labels: &model::common::Glabels) -> Option<String> {
    labels.items.iter().find_map(|item| {
        if let model::common::LabelChoice::Description(desc) = item {
            Some(desc.value.clone())
        } else {
            None
        }
    })
}

/// Resolves a `<combinedState>` model into the public type.
fn resolve_combined_state(
    model: &model::device_manager::CombinedState,
) -> Result<types::CombinedState, XdcError> {
    let led_state_refs = model
        .led_state_ref
        .iter()
        .map(|r| r.state_id_ref.clone())
        .collect();

    Ok(types::CombinedState {
        label: extract_label(&model.labels),
        description: extract_description(&model.labels),
        led_state_refs,
    })
}

/// ResolVes an `<LEDstate>` model into the public type.
fn resolve_led_state(
    model: &model::device_manager::LEDstate,
) -> Result<types::LEDstate, XdcError> {
    Ok(types::LEDstate {
        unique_id: model.unique_id.clone(),
        state: match model.state {
            model::device_manager::LEDstateEnum::On => "on".to_string(),
            model::device_manager::LEDstateEnum::Off => "off".to_string(),
            model::device_manager::LEDstateEnum::Flashing => "flashing".to_string(),
        },
        color: match model.led_color {
            model::device_manager::LEDcolor::Green => "green".to_string(),
            model::device_manager::LEDcolor::Amber => "amber".to_string(),
            model::device_manager::LEDcolor::Red => "red".to_string(),
        },
        label: extract_label(&model.labels),
        description: extract_description(&model.labels),
    })
}

/// Resolves an `<LED>` model into the public type.
fn resolve_led(model: &model::device_manager::LED) -> Result<types::LED, XdcError> {
    let states = model
        .led_state
        .iter()
        .map(resolve_led_state)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(types::LED {
        label: extract_label(&model.labels),
        description: extract_description(&model.labels),
        colors: match model.led_colors {
            model::device_manager::LEDcolors::Monocolor => "monocolor".to_string(),
            model::device_manager::LEDcolors::Bicolor => "bicolor".to_string(),
        },
        led_type: model.led_type.map(|t| match t {
            model::device_manager::LEDtype::Io => "IO".to_string(),
            model::device_manager::LEDtype::Device => "device".to_string(),
            model::device_manager::LEDtype::Communication => "communication".to_string(),
        }),
        states,
    })
}

/// Resolves an `<LEDList>` model into the public type.
fn resolve_led_list(
    model: &model::device_manager::LEDList,
) -> Result<types::IndicatorList, XdcError> {
    let leds = model
        .led
        .iter()
        .map(resolve_led)
        .collect::<Result<Vec<_>, _>>()?;

    let combined_states = model
        .combined_state
        .iter()
        .map(resolve_combined_state)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(types::IndicatorList {
        leds,
        combined_states,
    })
}

/// Resolves an `<indicatorList>` model into the public type.
fn resolve_indicator_list(
    model: &model::device_manager::IndicatorList,
) -> Result<types::IndicatorList, XdcError> {
    // The model has <indicatorList><LEDList>...
    // The public type just combines this.
    // Fix: Wrap return in Ok()
    Ok(model
        .led_list
        .as_ref()
        .map(resolve_led_list)
        .transpose()?
        .unwrap_or_default())
}

/// Parses a `model::DeviceManager` into a `types::DeviceManager`.
pub(super) fn resolve_device_manager(
    model: &model::device_manager::DeviceManager,
) -> Result<types::DeviceManager, XdcError> {
    let indicator_list = model
        .indicator_list
        .as_ref()
        .map(resolve_indicator_list)
        .transpose()?;

    // Resolve the modular device management part, if it exists
    // Fix: Correctly reference the field
    let module_management = model
        .module_management
        .as_ref()
        .map(modular::resolve_module_management_device)
        .transpose()?;

    Ok(types::DeviceManager {
        indicator_list,
        module_management,
    })
}