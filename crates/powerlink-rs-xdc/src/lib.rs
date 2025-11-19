// src/lib.rs

#![no_std]
#![doc = "Parses and generates POWERLINK XDC (XML Device Configuration) files."]
#![doc = ""]
#![doc = "This `no_std + alloc` library provides type-safe parsing and serialization"]
#![doc = "for POWERLINK XDC (Configuration Manager) data."]
#![doc = ""]
#![doc = "It supports:"]
#![doc = "- `load_xdc_from_str`: Parsing `actualValue` attributes from an XDC."]
#![doc = "- `load_xdd_defaults_from_str`: Parsing `defaultValue` attributes from an XDD."]
#![doc = "- `save_xdc_to_string`: Serializing configuration data back into a minimal XDC string."]

extern crate alloc;

// --- Crate Modules ---

mod builder;
mod converter;
mod error;
mod model;
mod parser;
mod resolver;
mod types;

// --- Public API Re-exports ---

// Functions
pub use builder::save_xdc_to_string;
pub use converter::{NmtSettings, extract_nmt_settings, to_core_od, xdc_to_storage_map};
pub use error::XdcError;
pub use parser::{load_xdc_from_str, load_xdd_defaults_from_str};

// Public Types
pub use types::{
    AddInfo, AllowedValues, AppArray, AppDataType, AppDerived, AppEnum, AppStruct,
    ApplicationProcess, Capabilities, Characteristic, CharacteristicList, Classification,
    CnFeatures, CombinedState, ConnectedModule, Connector, Count, DeviceFunction, DeviceManager,
    Diagnostic, Dictionary, EnumValue, ErrorDefinition, Firmware, FunctionInstance, FunctionType,
    GeneralFeatures, Identity, IndicatorList, InterfaceComm, InterfaceDevice, InterfaceList, LED,
    LEDstate, MnFeatures, ModuleInterface, ModuleManagementComm, ModuleManagementDevice,
    NetworkManagement, NmtCnDna, Object, ObjectDictionary, ObjectPdoMapping, ParameterAccess,
    ParameterGroup, ParameterGroupItem, ParameterRef, ParameterSupport, Picture, ProfileHeader,
    Range, StandardCompliance, StaticErrorBit, StructMember, SubObject, Value, ValueRange,
    VarDeclaration, Version, VersionInfo, XdcFile,
};
