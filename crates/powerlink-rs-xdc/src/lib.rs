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
mod converter; // Add the new module
mod error;
mod model;
mod parser;
mod resolver; // This now correctly refers to the `src/resolver/mod.rs` directory
mod types;

// --- Public API Re-exports ---

// Functions
pub use builder::save_xdc_to_string;
pub use converter::to_core_od; // Export the new converter function
pub use error::XdcError;
pub use parser::{load_xdc_from_str, load_xdd_defaults_from_str};

// Public Types
pub use types::{
    AddInfo, AppArray, AppDataType, AppDerived, AppEnum, AppStruct, ApplicationProcess, CnFeatures,
    Count, Diagnostic, EnumValue, ErrorDefinition, FunctionInstance, FunctionType,
    GeneralFeatures, Identity, InterfaceList, MnFeatures, NetworkManagement, NmtCnDna, Object,
    ObjectDictionary, ObjectPdoMapping, ParameterAccess, ParameterGroup, ParameterGroupItem,
    ParameterRef, ParameterSupport, ProfileHeader, StaticErrorBit, StructMember, SubObject,
    VarDeclaration, Version, VersionInfo, XdcFile,
};