# POWERLINK XDC Parser (`powerlink-rs-xdc`)

A `no_std` compatible, high-performance parser for ETHERNET POWERLINK XML Device Configuration (XDC) files, written in pure Rust.

This crate is part of the `powerlink-rs` project. It is designed to parse, validate, and provide an ergonomic, strongly-typed Rust API for accessing data from `.xdc` files, which are based on the [EPSG DS 311](https://www.br-automation.com/en/technologies/powerlink/service-downloads/technical-documents/) specification.

**Work in progress**.

## Features

- **`no_std` Compatible:** Can be used in embedded and bare-metal environments (`alloc` required).
- **High Performance:** Uses the event-based `quick-xml` parser to minimize allocations and efficiently handle large files.
- **Ergonomic API:** Translates raw XML data into strongly-typed Rust structs and enums.
- **Safe:** Built with safe Rust, with no `unwrap()` or `expect()` in library code.
- **Serialization:** Includes `save_xdc_to_string` to serialize configuration back into a standard XDC XML string.

## Architecture & Module Responsibilities

The crate is designed around a three-stage pipeline: **Parse -> Resolve -> Expose**. This separation of concerns allows for a robust, maintainable, and testable codebase.

[file.xdc] -> `parser.rs` -> `model/` -> `resolver/` -> `types.rs` -> `converter.rs` -> [Node]

- **`src/parser.rs` (Entry Point)**
  - **Responsibility:** The main entry point for parsing an XDC file.
  - **Details:** It takes the XML string content and uses `quick-xml`'s `from_str` deserializer. Its *only* job is to orchestrate the deserialization of the raw XML into the internal `model::XmlRoot` struct.
- **`src/model/` (Internal `serde` Model)**
  - **Responsibility:** Defines the raw, internal data structures that map 1:1 to the XDC XML schema.
  - **Details:** These structs are considered an implementation detail and are not exposed publicly. They are heavily annotated with `#[serde(...)]` attributes to guide `quick-xml`. Their goal is to capture the XML data as-is, including `String` representations of enums, hex values, etc.
- **`src/resolver.rs` (Business Logic)**
  - **Responsibility:** The "brains" of the crate. It converts the "dumb" `model` structs into the "smart" public `types` structs.
  - **Details:** This module contains all the business logic for parsing string values into enums, converting hex strings into `Vec<u8>`, validating data, and resolving data types (e.g., handling `uniqueIDRef` lookups between `ObjectList` and `ApplicationProcess`).
- **`src/types.rs` (Public API)**
  - **Responsibility:** Defines the public, ergonomic data structures that consumers of this crate will interact with.
  - **Details:** These structs are clean, well-documented, and use rich types (e.g., enums, `u16`) instead of strings.
- **`src/error.rs`**
  - **Responsibility:** Defines the crate's custom `XdcError` enum.
  - **Details:** Provides detailed error information, distinguishing between XML parsing errors (from `quick-xml`) and data resolution errors (e.g., "Invalid AccessType string").
- **`src/lib.rs`**
  - **Responsibility:** The main crate library entry point.
  - **Details:** Re-exports the public API from `src/types.rs` and the main `parse_xdc` function from `src/parser.rs`.
- **`src/builder.rs`**
  - **Responsibility:** Provides a `save_xdc_to_string` function for serializing a `types::XdcFile` struct back into XML.
  - **Details:** This is essential for tools that need to create or modify XDC files, not just read them.

## XDC Specification Coverage

This table tracks the crate's implementation status against the main features of the EPSG DS 311 specification.

| Feature / Element | XSD Definition | Status | Notes |
| :--- | :--- | :--- | :--- |
| **ProfileHeader** | `ProfileHeader_DataType` | 🟢 **Implemented** | All key fields modeled and resolved. |
| **ProfileBody** | `ProfileBody_DataType` | 🟢 **Implemented** | |
| ➡️ **DeviceIdentity** | `t_DeviceIdentity` | 🟢 **Implemented** | All fields from XSD are modeled and resolved. |
| ➡️ **ApplicationProcess** | `t_ApplicationProcess` | 🟡 **In Progress** | `parameterList` and `templateList` are fully modeled. Resolver correctly resolves attributes (`access`, `support`, `persistent`) and values via `uniqueIDRef`. `parameterGroupList` is not yet implemented. |
| ➡️ **ObjectList** | `ag_Powerlink_ObjectList` | 🟢 **Implemented** | Fully modeled and resolved, including `uniqueIDRef` resolution from `ApplicationProcess`. |
| ➡️ **Object** | `ag_Powerlink_Object` | 🟢 **Implemented** | All key attributes modeled and resolved. |
| ➡️ **SubObject** | `ag_Powerlink_Object` | 🟢 **Implemented** | All key attributes modeled and resolved. |
| ➡️ **NetworkManagement** | `t_NetworkManagement` | 🟢 **Implemented** | All key sub-elements modeled and resolved. |
| ➡️ **GeneralFeatures** | `t_GeneralFeatures` | 🟢 **Implemented** | Key features are modeled and resolved. |
| ➡️ **MNFeatures** | `t_MNFeatures` | 🟢 **Implemented** | Key features are modeled and resolved. |
| ➡️ **CNFeatures** | `t_CNFeatures` | 🟢 **Implemented** | Key features are modeled and resolved. |
| ➡️ **Diagnostic** | `t_Diagnostic` | 🟢 **Implemented** | `ErrorList` is modeled and resolved. |

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

- **Focus:** Implement parsing for the remaining sections of the XDC schema, primarily `NetworkManagement` and `ApplicationProcess`.
- **Key Features:**
  - Add `serde` models for `NetworkManagement`, `GeneralFeatures`, `MNFeatures`, `CNFeatures`, and `Diagnostic`.
  - Add public `types` for the `NetworkManagement` data.
  - Implement `resolver.rs` logic to map and validate this data.
  - Robustly model `ApplicationProcess` attributes and resolve them via `uniqueIDRef`.
- **Success Metric:** The crate can parse 100% of the elements and attributes defined in the EPSG DS 311 XSDs.
- **Status:** 🟡 **In Progress** (Core `ApplicationProcess` logic is complete. `parameterGroupList` and other minor elements are pending).

### Phase 3: Comprehensive Testing & Validation

- **Focus:** Ensure the parser is robust, compliant, and correct by testing against a wide variety of real-world and malformed XDC files.
- **Key Features:**
  - Integrate a test suite of diverse, valid XDC files from different vendors.
  - Create specification-driven unit tests for all resolver logic (e.g., `accessType` parsing, `PDOmapping` logic).
  - Develop fuzz tests to handle malformed or unexpected XML structures.
  - Add tests for edge-case data type parsing (e.g., `Unsigned24`, bit-packed structs).
- **Success Metric:** The crate achieves >95% test coverage and correctly parses all valid XDC files in the test suite while returning descriptive errors for all malformed ones.
- **Status:** 🔴 **Not Started**

### Phase 4: Serialization & Validation

- **Focus:** Provide ergonomic data creation tools and full serialization.
- **Key Features:**
  - Implement `quick-xml` serialization to write an `XdcFile` struct back to an XML string.
  - Add a high-level `validate()` method to `XdcFile` that checks for common *semantic* configuration errors (e.g., invalid PDO mappings).
  - Implement a `builder.rs` API for programmatically creating new `XdcFile` structs.
- **Success Metric:** A user can create a valid XDC file from scratch, serialize it to XML, parse it back, and get an identical struct.
- **Status:** 🟡 **In Progress** (`save_xdc_to_string` and `to_core_od` converter are implemented).
