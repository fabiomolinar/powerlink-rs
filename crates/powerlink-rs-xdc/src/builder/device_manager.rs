// crates/powerlink-rs-xdc/src/builder/device_manager.rs

//! Contains builder functions to convert `types::DeviceManager` into `model::DeviceManager`.

use crate::model::common::{Description, Glabels, Label, LabelChoice};
use crate::{model, types};
use alloc::{string::ToString, vec};

/// Helper to convert public `types::LEDstate` to `model::device_manager::LEDstate`.
fn build_model_led_state(public: &types::LEDstate) -> model::device_manager::LEDstate {
    model::device_manager::LEDstate {
        labels: Glabels {
            items: vec![
                LabelChoice::Label(Label {
                    lang: "en".to_string(), // Default lang
                    value: public.label.clone().unwrap_or_default(),
                }),
                LabelChoice::Description(Description {
                    lang: "en".to_string(), // Default lang
                    value: public.description.clone().unwrap_or_default(),
                    ..Default::default()
                }),
            ],
        },
        unique_id: public.unique_id.clone(),
        state: match public.state.as_str() {
            "on" => model::device_manager::LEDstateEnum::On,
            "off" => model::device_manager::LEDstateEnum::Off,
            "flashing" => model::device_manager::LEDstateEnum::Flashing,
            _ => model::device_manager::LEDstateEnum::Off, // Default
        },
        led_color: match public.color.as_str() {
            "green" => model::device_manager::LEDcolor::Green,
            "amber" => model::device_manager::LEDcolor::Amber,
            "red" => model::device_manager::LEDcolor::Red,
            _ => model::device_manager::LEDcolor::Green, // Default
        },
        ..Default::default()
    }
}

/// Helper to convert public `types::LED` to `model::device_manager::LED`.
fn build_model_led(public: &types::LED) -> model::device_manager::LED {
    model::device_manager::LED {
        labels: Glabels {
            items: vec![
                LabelChoice::Label(Label {
                    lang: "en".to_string(),
                    value: public.label.clone().unwrap_or_default(),
                }),
                LabelChoice::Description(Description {
                    lang: "en".to_string(),
                    value: public.description.clone().unwrap_or_default(),
                    ..Default::default()
                }),
            ],
        },
        led_colors: match public.colors.as_str() {
            "monocolor" => model::device_manager::LEDcolors::Monocolor,
            "bicolor" => model::device_manager::LEDcolors::Bicolor,
            _ => model::device_manager::LEDcolors::Monocolor,
        },
        led_type: public.led_type.as_ref().map(|t| match t.as_str() {
            "IO" => model::device_manager::LEDtype::Io,
            "device" => model::device_manager::LEDtype::Device,
            "communication" => model::device_manager::LEDtype::Communication,
            _ => model::device_manager::LEDtype::Device, // Default
        }),
        led_state: public.states.iter().map(build_model_led_state).collect(),
    }
}

/// Helper to convert public `types::CombinedState` to `model::device_manager::CombinedState`.
fn build_model_combined_state(
    public: &types::CombinedState,
) -> model::device_manager::CombinedState {
    model::device_manager::CombinedState {
        labels: Glabels {
            items: vec![
                LabelChoice::Label(Label {
                    lang: "en".to_string(),
                    value: public.label.clone().unwrap_or_default(),
                }),
                LabelChoice::Description(Description {
                    lang: "en".to_string(),
                    value: public.description.clone().unwrap_or_default(),
                    ..Default::default()
                }),
            ],
        },
        led_state_ref: public
            .led_state_refs
            .iter()
            .map(|r| model::device_manager::LEDstateRef {
                state_id_ref: r.clone(),
            })
            .collect(),
    }
}

/// Converts a public `types::DeviceManager` into a `model::DeviceManager`.
pub(super) fn build_model_device_manager(
    public: &types::DeviceManager,
) -> model::device_manager::DeviceManager {
    let indicator_list =
        public
            .indicator_list
            .as_ref()
            .map(|indicators| model::device_manager::IndicatorList {
                led_list: Some(model::device_manager::LEDList {
                    led: indicators.leds.iter().map(build_model_led).collect(),
                    combined_state: indicators
                        .combined_states
                        .iter()
                        .map(build_model_combined_state)
                        .collect(),
                }),
            });

    let module_management = public
        .module_management
        .as_ref()
        .map(|mgmt| super::modular::build_model_module_management_device(mgmt));

    model::device_manager::DeviceManager {
        indicator_list,
        module_management,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types;
    use alloc::vec;

    #[test]
    fn test_build_model_device_manager() {
        // 1. Create public types
        let public_led_state = types::LEDstate {
            unique_id: "uid_led_state_1".to_string(),
            state: "flashing".to_string(),
            color: "red".to_string(),
            label: Some("Error".to_string()),
            description: Some("A critical error occurred".to_string()),
        };

        let public_led = types::LED {
            label: Some("STATUS".to_string()),
            description: None,
            colors: "bicolor".to_string(),
            led_type: Some("device".to_string()),
            states: vec![public_led_state],
        };

        let public_combined_state = types::CombinedState {
            label: Some("Combined Error".to_string()),
            description: None,
            led_state_refs: vec!["uid_led_state_1".to_string()],
        };

        let public_dm = types::DeviceManager {
            indicator_list: Some(types::IndicatorList {
                leds: vec![public_led],
                combined_states: vec![public_combined_state],
            }),
            module_management: None, // Tested in modular::tests
        };

        // 2. Call the builder
        let model_dm = build_model_device_manager(&public_dm);

        // 3. Verify the model
        let model_indicator_list = model_dm.indicator_list.unwrap();
        let model_led_list = model_indicator_list.led_list.unwrap();

        // Check LED
        assert_eq!(model_led_list.led.len(), 1);
        let model_led = &model_led_list.led[0];
        assert_eq!(
            model_led.led_colors,
            model::device_manager::LEDcolors::Bicolor
        );
        assert_eq!(
            model_led.led_type,
            Some(model::device_manager::LEDtype::Device)
        );

        // Check LEDstate
        assert_eq!(model_led.led_state.len(), 1);
        let model_state = &model_led.led_state[0];
        assert_eq!(model_state.unique_id, "uid_led_state_1");
        assert_eq!(
            model_state.state,
            model::device_manager::LEDstateEnum::Flashing
        );
        assert_eq!(model_state.led_color, model::device_manager::LEDcolor::Red);

        // Check CombinedState
        assert_eq!(model_led_list.combined_state.len(), 1);
        let model_combined = &model_led_list.combined_state[0];
        assert_eq!(model_combined.led_state_ref.len(), 1);
        assert_eq!(
            model_combined.led_state_ref[0].state_id_ref,
            "uid_led_state_1"
        );
    }
}
