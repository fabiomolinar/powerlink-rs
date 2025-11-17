// crates/powerlink-rs-xdc/src/model/device_manager.rs

//! Contains model structs related to `<DeviceManager>`.
//! (Schema: `ProfileBody_Device_Powerlink.xsd`)

use serde::{Deserialize, Serialize};
use alloc::vec::Vec;
use alloc::string::String;
use super::common::Glabels;
use super::modular::ModuleManagementDevice; // Import modular struct
use core::fmt; // Import for Display trait

/// Represents the `<DeviceManager>` element.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct DeviceManager {
    #[serde(rename = "indicatorList", default, skip_serializing_if = "Option::is_none")]
    pub indicator_list: Option<IndicatorList>,
    /// This field is only present in Modular Head device profiles.
    /// (from `ProfileBody_Device_Powerlink_Modular_Head.xsd`)
    #[serde(rename = "moduleManagement", default, skip_serializing_if = "Option::is_none")]
    pub module_management: Option<ModuleManagementDevice>,
}

/// Represents `<indicatorList>`.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct IndicatorList {
    #[serde(rename = "LEDList", default, skip_serializing_if = "Option::is_none")]
    pub led_list: Option<LEDList>,
}

/// Represents `<LEDList>`.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct LEDList {
    #[serde(rename = "LED", default, skip_serializing_if = "Vec::is_empty")]
    pub led: Vec<LED>,
    #[serde(rename = "combinedState", default, skip_serializing_if = "Vec::is_empty")]
    pub combined_state: Vec<CombinedState>,
}

/// Represents the `@LEDcolors` attribute enum.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum LEDcolors {
    #[serde(rename = "monocolor")]
    Monocolor,
    #[serde(rename = "bicolor")]
    Bicolor,
}

impl fmt::Display for LEDcolors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LEDcolors::Monocolor => write!(f, "monocolor"),
            LEDcolors::Bicolor => write!(f, "bicolor"),
        }
    }
}

// Fix: Add Default implementation
impl Default for LEDcolors {
    fn default() -> Self {
        Self::Monocolor
    }
}

/// Represents the `@LEDtype` attribute enum.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum LEDtype {
    #[serde(rename = "IO")]
    Io,
    #[serde(rename = "device")]
    Device,
    #[serde(rename = "communication")]
    Communication,
}

impl fmt::Display for LEDtype {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LEDtype::Io => write!(f, "IO"),
            LEDtype::Device => write!(f, "device"),
            LEDtype::Communication => write!(f, "communication"),
        }
    }
}

/// Represents an `<LED>` element.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct LED {
    #[serde(flatten)]
    pub labels: Glabels,
    #[serde(rename = "@LEDcolors")]
    pub led_colors: LEDcolors,
    #[serde(rename = "@LEDtype", default, skip_serializing_if = "Option::is_none")]
    pub led_type: Option<LEDtype>,
    #[serde(rename = "LEDstate", default, skip_serializing_if = "Vec::is_empty")]
    pub led_state: Vec<LEDstate>,
}

/// Represents the `@state` attribute enum.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum LEDstateEnum {
    #[serde(rename = "on")]
    On,
    #[serde(rename = "off")]
    Off,
    #[serde(rename = "flashing")]
    Flashing,
}

impl fmt::Display for LEDstateEnum {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LEDstateEnum::On => write!(f, "on"),
            LEDstateEnum::Off => write!(f, "off"),
            LEDstateEnum::Flashing => write!(f, "flashing"),
        }
    }
}

// Fix: Add Default implementation
impl Default for LEDstateEnum {
    fn default() -> Self {
        Self::Off
    }
}

/// Represents the `@LEDcolor` attribute enum.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum LEDcolor {
    #[serde(rename = "green")]
    Green,
    #[serde(rename = "amber")]
    Amber,
    #[serde(rename = "red")]
    Red,
}

impl fmt::Display for LEDcolor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LEDcolor::Green => write!(f, "green"),
            LEDcolor::Amber => write!(f, "amber"),
            LEDcolor::Red => write!(f, "red"),
        }
    }
}

// Fix: Add Default implementation
impl Default for LEDcolor {
    fn default() -> Self {
        Self::Green
    }
}

/// Represents an `<LEDstate>` element.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct LEDstate {
    #[serde(flatten)]
    pub labels: Glabels,
    #[serde(rename = "@uniqueID")]
    pub unique_id: String, // xsd:ID
    #[serde(rename = "@state")]
    pub state: LEDstateEnum,
    #[serde(rename = "@LEDcolor")]
    pub led_color: LEDcolor,
    #[serde(rename = "@flashingPeriod", default, skip_serializing_if = "Option::is_none")]
    pub flashing_period: Option<String>, // xsd:unsignedInt
    #[serde(rename = "@impulsWidth", default, skip_serializing_if = "Option::is_none")]
    pub impuls_width: Option<String>, // xsd:unsignedByte, default "50"
    #[serde(rename = "@numberOfImpulses", default, skip_serializing_if = "Option::is_none")]
    pub number_of_impulses: Option<String>, // xsd:unsignedByte, default "1"
}

/// Represents an `<LEDstateRef>` element.
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct LEDstateRef {
    #[serde(rename = "@stateIDRef")]
    pub state_id_ref: String, // xsd:IDREF
}

/// Represents a `<combinedState>` element.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct CombinedState {
    #[serde(flatten)]
    pub labels: Glabels,
    #[serde(rename = "LEDstateRef", default, skip_serializing_if = "Vec::is_empty")]
    pub led_state_ref: Vec<LEDstateRef>, // minOccurs="2"
}