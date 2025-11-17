// crates/powerlink-rs-xdc/src/resolver/device_manager.rs

//! Handles resolving the `<DeviceManager>` block from the model to public types.

use crate::error::XdcError;
use crate::model;
use crate::resolver::modular; // Import the new modular resolver
use crate::resolver::utils; // Import the utils module
use crate::types;
use alloc::string::{String, ToString}; // Fix: Add String import
use alloc::vec::Vec;

// --- Label Helpers ---

// REMOVED: `extract_label` - Now in `utils.rs`
// REMOVED: `extract_description` - Now in `utils.rs`

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
        label: utils::extract_label(&model.labels), // Use utils::
        description: utils::extract_description(&model.labels), // Use utils::
        led_state_refs,
    })
}

/// ResolVes an `<LEDstate>` model into the public type.
fn resolve_led_state(model: &model::device_manager::LEDstate) -> Result<types::LEDstate, XdcError> {
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
        label: utils::extract_label(&model.labels), // Use utils::
        description: utils::extract_description(&model.labels), // Use utils::
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
        label: utils::extract_label(&model.labels), // Use utils::
        description: utils::extract_description(&model.labels), // Use utils::
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        common::{Glabels, Label, LabelChoice},
        device_manager as model_dm, modular as model_mod,
    };
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

    // --- UNIT TESTS ---

    #[test]
    fn test_resolve_led_state() {
        let model_state = model_dm::LEDstate {
            labels: Glabels {
                items: vec![create_test_label("Operational")],
            },
            unique_id: "led1_op".to_string(),
            state: model_dm::LEDstateEnum::On,
            led_color: model_dm::LEDcolor::Green,
            ..Default::default()
        };

        let pub_state = resolve_led_state(&model_state).unwrap();
        assert_eq!(pub_state.unique_id, "led1_op");
        assert_eq!(pub_state.state, "on");
        assert_eq!(pub_state.color, "green");
        assert_eq!(pub_state.label, Some("Operational".to_string()));
    }

    #[test]
    fn test_resolve_led() {
        let model_led = model_dm::LED {
            labels: Glabels {
                items: vec![create_test_label("STATUS")],
            },
            led_colors: model_dm::LEDcolors::Bicolor,
            led_type: Some(model_dm::LEDtype::Device),
            led_state: vec![
                model_dm::LEDstate {
                    unique_id: "s1".to_string(),
                    state: model_dm::LEDstateEnum::On,
                    led_color: model_dm::LEDcolor::Green,
                    ..Default::default()
                },
                model_dm::LEDstate {
                    unique_id: "s2".to_string(),
                    state: model_dm::LEDstateEnum::Flashing,
                    led_color: model_dm::LEDcolor::Red,
                    ..Default::default()
                },
            ],
        };

        let pub_led = resolve_led(&model_led).unwrap();
        assert_eq!(pub_led.label, Some("STATUS".to_string()));
        assert_eq!(pub_led.colors, "bicolor");
        assert_eq!(pub_led.led_type, Some("device".to_string()));
        assert_eq!(pub_led.states.len(), 2);
        assert_eq!(pub_led.states[0].unique_id, "s1");
        assert_eq!(pub_led.states[1].color, "red");
    }

    #[test]
    fn test_resolve_combined_state() {
        let model_cs = model_dm::CombinedState {
            labels: Glabels {
                items: vec![create_test_label("Error Stop")],
            },
            led_state_ref: vec![
                model_dm::LEDstateRef {
                    state_id_ref: "led1_red".to_string(),
                },
                model_dm::LEDstateRef {
                    state_id_ref: "led2_flashing".to_string(),
                },
            ],
        };

        let pub_cs = resolve_combined_state(&model_cs).unwrap();
        assert_eq!(pub_cs.label, Some("Error Stop".to_string()));
        assert_eq!(pub_cs.led_state_refs.len(), 2);
        assert_eq!(pub_cs.led_state_refs[0], "led1_red");
    }

    #[test]
    fn test_resolve_indicator_list() {
        let model_il = model_dm::IndicatorList {
            led_list: Some(model_dm::LEDList {
                led: vec![model_dm::LED {
                    led_colors: model_dm::LEDcolors::Monocolor,
                    ..Default::default()
                }],
                combined_state: vec![model_dm::CombinedState {
                    led_state_ref: vec![model_dm::LEDstateRef::default(); 2],
                    ..Default::default()
                }],
            }),
        };

        let pub_il = resolve_indicator_list(&model_il).unwrap();
        assert_eq!(pub_il.leds.len(), 1);
        assert_eq!(pub_il.combined_states.len(), 1);
    }

    #[test]
    fn test_resolve_device_manager() {
        let model_dm = model_dm::DeviceManager {
            indicator_list: Some(model_dm::IndicatorList {
                led_list: Some(model_dm::LEDList {
                    led: vec![model_dm::LED {
                        led_colors: model_dm::LEDcolors::Monocolor,
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
            }),
            module_management: Some(model_mod::ModuleManagementDevice {
                interface_list: model_mod::InterfaceListDevice {
                    interface: vec![model_mod::InterfaceDevice {
                        unique_id: "if1".to_string(),
                        interface_type: "X2X".to_string(),
                        max_modules: "10".to_string(),
                        module_addressing: model_mod::ModuleAddressingHead::Position,
                        file_list: model_mod::FileList {
                            file: vec![model_mod::File {
                                uri: "test.xdd".to_string(),
                            }],
                        },
                        ..Default::default()
                    }],
                },
                ..Default::default()
            }),
        };

        let pub_dm = resolve_device_manager(&model_dm).unwrap();

        // Check indicatorList
        assert!(pub_dm.indicator_list.is_some());
        assert_eq!(pub_dm.indicator_list.unwrap().leds.len(), 1);

        // Check moduleManagement
        assert!(pub_dm.module_management.is_some());
        let pub_mm = pub_dm.module_management.unwrap();
        assert_eq!(pub_mm.interfaces.len(), 1);
        assert_eq!(pub_mm.interfaces[0].unique_id, "if1");
        assert_eq!(pub_mm.interfaces[0].max_modules, 10);
        assert_eq!(pub_mm.interfaces[0].file_list.len(), 1);
    }

    #[test]
    fn test_resolve_indicator_list_default() {
        // Test that a default (empty) indicator list resolves to a default (empty) public type
        let pub_il = resolve_indicator_list(&model_dm::IndicatorList::default()).unwrap();
        assert_eq!(pub_il, types::IndicatorList::default());
    }
}