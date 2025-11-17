// crates/powerlink-rs-xdc/src/resolver/device_manager.rs

//! Handles resolving the `<DeviceManager>` block from the model to public types.

use crate::error::XdcError;
use crate::model;
use crate::resolver::{modular, utils}; // Import the utils module
use crate::types;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::model::device_manager as model_dm;
use crate::model::modular as model_mod;

// --- Sub-Resolvers ---

/// Resolves an `<LEDstate>`.
fn resolve_led_state(model: &model_dm::LEDstate) -> Result<types::LEDstate, XdcError> {
    Ok(types::LEDstate {
        unique_id: model.unique_id.clone(),
        state: model.state.to_string(),
        color: model.led_color.to_string(),
        // FIX: Removed fields that are not in the public `types::LEDState` struct
        // flashing_period: model
        //     .flashing_period
        //     .as_ref()
        //     .and_then(|p| p.parse().ok()),
        // impuls_width: model
        //     .impuls_width
        //     .as_ref()
        //     .and_then(|w| w.parse().ok()),
        // number_of_impulses: model
        //     .number_of_impulses
        //     .as_ref()
        //     .and_then(|n| n.parse().ok()),
        label: utils::extract_label(&model.labels.items), // Use utils::
        description: utils::extract_description(&model.labels.items), // Use utils::
    })
}

/// Resolves an `<LED>`.
fn resolve_led(model: &model_dm::LED) -> Result<types::LED, XdcError> {
    let states = model
        .led_state
        .iter()
        .map(resolve_led_state)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(types::LED {
        led_type: model.led_type.map(|t| t.to_string()),
        colors: model.led_colors.to_string(),
        label: utils::extract_label(&model.labels.items), // Use utils::
        description: utils::extract_description(&model.labels.items), // Use utils::
        states,
    })
}

/// Resolves a `<combinedState>`.
fn resolve_combined_state(model: &model_dm::CombinedState) -> Result<types::CombinedState, XdcError> {
    Ok(types::CombinedState {
        led_state_refs: model
            .led_state_ref
            .iter()
            .map(|r| r.state_id_ref.clone())
            .collect(),
        label: utils::extract_label(&model.labels.items), // Use utils::
        description: utils::extract_description(&model.labels.items), // Use utils::
    })
}

/// Resolves an `<indicatorList>`.
fn resolve_indicator_list(
    model: &model_dm::IndicatorList,
) -> Result<types::IndicatorList, XdcError> {
    let leds = model
        .led_list
        .as_ref()
        .map_or(Ok(Vec::new()), |list| {
            list.led.iter().map(resolve_led).collect()
        })?;

    let combined_states = model
        .led_list
        .as_ref()
        .map_or(Ok(Vec::new()), |list| {
            list.combined_state.iter().map(resolve_combined_state).collect()
        })?;

    Ok(types::IndicatorList {
        leds,
        combined_states,
    })
}

// --- Main Resolver ---

/// Parses a `model::DeviceManager` into a `types::DeviceManager`.
pub(super) fn resolve_device_manager(
    model: &model_dm::DeviceManager,
) -> Result<types::DeviceManager, XdcError> {
    let indicator_list = model
        .indicator_list
        .as_ref()
        .map(resolve_indicator_list)
        .transpose()?;

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