#![no_std]
#![doc = "Parses and generates POWERLINK XDC (XML Device Configuration) files."]
#![doc = ""]
#![doc = "This `no_std + alloc` library provides type-safe parsing and serialization"]
#![doc = "for POWERLINK XDC (Configuration Manager) data based on the EPSG DS 311 specification."]
#![doc = ""]
#![doc = "It provides the following main capabilities:"]
#![doc = "- **Parsing**: Loading `.xdc` or `.xdd` XML strings into strongly-typed Rust structures."]
#![doc = "- **Resolution**: Resolving complex inheritance, `uniqueIDRef` links, and templates defined in the Application Process."]
#![doc = "- **Serialization**: generating minimal valid XDC XML strings from Rust structures."]
#![doc = "- **Core Integration**: Converting the parsed data into the `ObjectDictionary` format required by the `powerlink-rs` core stack."]

extern crate alloc;

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
pub use converter::{extract_nmt_settings, to_core_od, xdc_to_storage_map, NmtSettings};
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