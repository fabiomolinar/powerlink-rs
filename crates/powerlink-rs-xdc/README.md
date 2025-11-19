# POWERLINK XDC Parser (`powerlink-rs-xdc`)

A `no_std` compatible, high-performance parser and serializer for ETHERNET POWERLINK XML Device Configuration (XDC) files, written in pure Rust.

This crate is part of the `powerlink-rs` project. It is designed to parse, validate, provide an ergonomic, strongly-typed Rust API for accessing data from `.xdc` files, and serialize that data back to XML. It is based on the [EPSG DS 311](https://www.br-automation.com/en/technologies/powerlink/service-downloads/technical-documents/) specification.

**Work in progress**.

## Features

- **`no_std` Compatible:** Can be used in embedded and bare-metal environments (`alloc` required).
- **High Performance:** Uses the event-based `quick-xml` parser to minimize allocations and efficiently handle large files.
- **Ergonomic API:** Translates raw XML data into strongly-typed Rust structs and enums.
- **Safe:** Built with safe Rust, with no `unwrap()` or `expect()` in library code.
- **Full Serialization:** Includes `save_xdc_to_string` to serialize a complete `XdcFile` struct back into a standard XDC XML string.
- **Core Crate Integration:** Provides a `to_core_od` converter to directly build the `ObjectDictionary` required by the `powerlink-rs` core crate.
- **Full Schema Support:** Parses and resolves:
  - `DeviceIdentity`
  - `DeviceManager` (including LEDs and modular support)
  - `ApplicationProcess` (including `parameterList`, `dataTypeList`, etc.)
  - `ObjectList` (the Object Dictionary)
  - `NetworkManagement` (including `GeneralFeatures`, `MNFeatures`, `CNFeatures`, `Diagnostic`)
  - **Modular Device Profiles** (XDDM) for both Head and Child nodes.

## Usage

```rust
use powerlink_rs_xdc::{load_xdc_from_str, to_core_od};
use std::fs;

fn main() {
    // 1. Load the XDD/XDC file content
    let xml_content = fs::read_to_string("tests/data/MyDevice.xdd")
        .expect("Failed to read file");

    // 2. Parse into a strongly-typed XdcFile struct
    //    (Use load_xdd_defaults_from_str for XDDs to prioritize defaultValue)
    let xdc_file = load_xdc_from_str(&xml_content)
        .expect("Failed to parse XDC");

    println!("Device: {}", xdc_file.identity.product_name);
    println!("Vendor ID: {:#010x}", xdc_file.identity.vendor_id);

    // 3. Access the Object Dictionary
    if let Some(obj) = xdc_file.object_dictionary.objects.iter().find(|o| o.index == 0x1000) {
        println!("Device Type: {:?}", obj.data);
    }

    // 4. Convert to the core crate's ObjectDictionary format (if needed)
    let core_od = to_core_od(&xdc_file)
        .expect("Failed to convert to core OD");
}
```

## Data Representation & Endianness

**Important Design Decision:**

While the POWERLINK protocol transmits data in **Little Endian** byte order, this crate treats all data within XDC/XDD files as **human-readable strings**.

- **Storage:** Values in `types.rs` (e.g., `Object::data`, `Parameter::actual_value`) are stored as `String` (e.g., `"0x1234"`, `"500"`).
- **Parsing:** The parser does *not* convert these strings into byte vectors or native integers during the initial load. This ensures full fidelity to the XML source (preserving hex vs decimal formatting).
- **Conversion:** Conversion to native Rust types (and subsequently to Little Endian bytes for the network) occurs exclusively in the `converter.rs` module when transforming the data for the `powerlink-rs` core crate.

This approach simplifies round-trip serialization (ensuring `save_xdc_to_string` produces XML that matches the input style) and decouples XML formatting from protocol-specific byte ordering.

## Architecture & Module Responsibilities

The crate is designed around a three-stage pipeline: **Parse -> Resolve -> Expose**, with additional modules for serialization and conversion.

[file.xdc] -> `parser.rs` -> `model/` -> `resolver/` -> `types.rs` -> [Consumer]
[types.rs] -> `converter.rs` -> [powerlink-rs core]
[types.rs] -> `builder/` -> [file.xdc]

- **`src/parser.rs` (Entry Point)**
  - **Responsibility:** The main entry point for parsing an XDC file.
  - **Details:** It takes the XML string content and uses `quick-xml`'s `from_str` deserializer. Its *only* job is to orchestrate the deserialization of the raw XML into the internal `model` structs.
- **`src/model/` (Internal `serde` Model)**
  - **Responsibility:** Defines the raw, internal data structures that map 1:1 to the XDC XML schema.
  - **Details:** These structs are considered an implementation detail and are not exposed publicly. They are heavily annotated with `#[serde(...)]` attributes to guide `quick-xml`. Their goal is to capture the XML data as-is, including `String` representations of enums, hex values, etc.
- **`src/resolver/` (Business Logic)**
  - **Responsibility:** The "brains" of the crate. It converts the "dumb" `model` structs into the "smart" public `types` structs.
  - **Details:** This module contains all the business logic for parsing string values into enums, resolving data types (e.g., handling `uniqueIDRef` lookups between `ObjectList` and `ApplicationProcess`), and passing value strings through.
- **`src/types.rs` (Public API)**
  - **Responsibility:** Defines the public, ergonomic data structures that consumers of this crate will interact with.
  - **Details:** These structs are clean, well-documented, and use rich types (e.g., enums) instead of strings where applicable, while keeping data values as human-readable strings.
- **`src/converter.rs` (Core Integration)**
  - **Responsibility:** Translates the public `types::ObjectDictionary` into the `powerlink_rs::od::ObjectDictionary` used by the core `powerlink-rs` crate. This is where string-to-numeric parsing occurs.
- **`src/builder/` (Serialization)**
  - **Responsibility:** Provides a `save_xdc_to_string` function for serializing a `types::XdcFile` struct back into XML.
  - **Details:** This module converts the public `types` structs back into the internal `model` structs for serialization by `quick-xml`.
- **`src/error.rs`**
  - **Responsibility:** Defines the crate's custom `XdcError` enum.
  - **Details:** Provides detailed error information, distinguishing between XML parsing errors (from `quick-xml`) and data resolution errors (e.g., "Invalid AccessType string").
- **`src/lib.rs`**
  - **Responsibility:** The main crate library entry point.
  - **Details:** Re-exports the public API from `src/types.rs` and the main `load_` and `save_` functions.

## XDC Specification Coverage

This table tracks the crate's implementation status against the main features of the EPSG DS 311 specification.

| Feature / Element | XSD Definition | Status | Notes |
| :--- | :--- | :--- | :--- |
| **ProfileHeader** | `ProfileHeader_DataType` | 🟢 **Implemented** | All key fields modeled and resolved. |
| **ProfileBody** | `ProfileBody_DataType` | 🟢 **Implemented** | |
| ➡️ **DeviceIdentity** | `t_DeviceIdentity` | 🟢 **Implemented** | All fields from XSD are modeled and resolved. |
| ➡️ **DeviceManager** | `t_DeviceManager` | 🟢 **Implemented** | `indicatorList` (LEDs) and modular `moduleManagement` are modeled and resolved. |
| ➡️ **ApplicationProcess** | `t_ApplicationProcess` | 🟢 **Implemented** | All major sub-elements (`parameterList`, `dataTypeList`, `parameterGroupList`, `functionTypeList`, `functionInstanceList`) are modeled and resolved. |
| ➡️ **ObjectList** | `ag_Powerlink_ObjectList` | 🟢 **Implemented** | Fully modeled and resolved, including `uniqueIDRef` resolution from `ApplicationProcess`. |
| ➡️ **Object** | `ag_Powerlink_Object` | 🟢 **Implemented** | All key attributes modeled and resolved. |
| ➡️ **SubObject** | `ag_Powerlink_Object` | 🟢 **Implemented** | All key attributes modeled and resolved. |
| ➡️ **NetworkManagement** | `t_NetworkManagement` | 🟢 **Implemented** | All key sub-elements modeled and resolved. |
| ➡️ **GeneralFeatures** | `t_GeneralFeatures` | 🟢 **Implemented** | Key features are modeled and resolved. |
| ➡️ **MNFeatures** | `t_MNFeatures` | 🟢 **Implemented** | Key features are modeled and resolved. |
| ➡️ **CNFeatures** | `t_CNFeatures` | 🟢 **Implemented** | Key features are modeled and resolved. |
| ➡️ **Diagnostic** | `t_Diagnostic` | 🟢 **Implemented** | `ErrorList` and `StaticErrorBitField` are modeled and resolved. |
| **Modular Support** | `*Modular_Head.xsd` | 🟢 **Implemented** | All modular profile bodies, `moduleManagement`, `interfaceList`, and `rangeList` elements are modeled and resolved. |

### Limitations

While the crate covers most of the EPSG DS 311 V1.2.1 specification and is sufficient for most standard device and communication profiles, certain optional or legacy elements are not yet implemented.

The parser is designed to be safe; it will ignore these unsupported elements rather than crashing.

## Roadmap

### Phase 1: Core Model & API

- **Focus:** Establish the 3-stage architecture and parse the most critical 80% of XDC data: the Object Dictionary.
- **Key Features:**
  - Complete `serde` models for `ProfileHeader`.
  - Complete `serde` models for `Object` and `SubObject`, including all attributes (`name`, `accessType`, `PDOmapping`, `objFlags`, etc.).
  - Complete `serde` models for `DeviceIdentity`.
- **Success Metric:** The crate can successfully parse a real-world XDC file and provide full, typed access to its entire Object Dictionary and Device Identity.
- **Status:** 🟢 **Complete**

### Phase 2: Full Specification Compliance

- **Focus:** Implement parsing for the remaining sections of the XDC schema, primarily `NetworkManagement`, `ApplicationProcess`, and modular device extensions.
- **Key Features:**
  - Add `serde` models for `NetworkManagement`, `GeneralFeatures`, `MNFeatures`, `CNFeatures`, and `Diagnostic`.
  - Add `serde` models for `DeviceManager` and all modular profile extensions (`moduleManagement`, `interfaceList`, `rangeList`).
  - Add public `types` for all new data.
  - Implement `resolver.rs` logic to map and validate all new data.
- **Success Metric:** The crate can parse 100% of the elements and attributes defined in the EPSG DS 311 XSDs, including modular device profiles.
- **Status:** 🟢 **Complete**

### Phase 3: Comprehensive Testing & Validation

- **Focus:** Ensure the parser is robust, compliant, and correct by testing against robustness and edge cases.
- **Key Features:**
  - Integrate a test suite of valid XDC files.
  - Create specification-driven unit tests for all resolver logic (e.g., `accessType` parsing, `PDOmapping` logic).
  - Develop fuzz tests to handle malformed or unexpected XML structures.
  - Add tests for edge-case data type parsing (e.g., `Unsigned24`, bit-packed structs).
- **Success Metric:** The crate achieves high test coverage on edge cases and returns descriptive errors for malformed inputs without panicking.
- **Status:** 🟢 **Complete**

### Phase 4: Serialization & Validation

- **Focus:** Provide ergonomic data creation tools and full serialization.
- **Key Features:**
  - Implement `quick-xml` serialization to write an `XdcFile` struct back to an XML string.
  - Add a high-level `validate()` method to `XdcFile` that checks for common *semantic* configuration errors (e.g., invalid PDO mappings).
  - Implement a `builder.rs` API for programmatically creating new `XdcFile` structs.
- **Success Metric:** A user can create a valid XDC file from scratch, serialize it to XML, parse it back, and get an identical struct.
- **Status:** 🟢 **Complete** (`save_xdc_to_string` and `to_core_od` converter are implemented).
