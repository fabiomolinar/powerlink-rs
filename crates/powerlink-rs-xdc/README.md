# POWERLINK XDC Parser (`powerlink-rs-xdc`)

A `no_std` compatible, high-performance parser for ETHERNET POWERLINK XML Device Configuration (XDC) files, written in pure Rust.

This crate is part of the `powerlink-rs` project. It is designed to parse, validate, and provide an ergonomic, strongly-typed Rust API for accessing data from `.xdc` files, which are based on the [EPSG DS 311](https://www.br-automation.com/en/technologies/powerlink/service-downloads/technical-documents/) specification.

**Work in progress**.

## Features

- **`no_std` Compatible:** Can be used in embedded and bare-metal environments (`alloc` required).
- **High Performance:** Uses the event-based `quick-xml` parser to minimize allocations and efficiently handle large files.
- **Ergonomic API:** Translates raw XML data into strongly-typed Rust structs and enums from the `powerlink-rs` crate (e.g., `AccessType`, `DataType`).
- **Safe:** Built with safe Rust, with no `unwrap()` or `expect()` in library code.

## Architecture & Module Responsibilities

The crate is designed around a three-stage pipeline: **Parse -> Resolve -> Expose**. This separation of concerns allows for a robust, maintainable, and testable codebase.

[file.xdc] -> parser.rs -> model.rs -> resolver.rs -> types.rs -> [Consumer]

- **`src/parser.rs` (Entry Point)**
  - **Responsibility:** The main entry point for parsing an XDC file.
  - **Details:** It takes the XML string content and uses `quick-xml`'s `from_str` deserializer. Its *only* job is to orchestrate the deserialization of the raw XML into the internal `model::XmlRoot` struct.
- **`src/model.rs` (Internal `serde` Model)**
  - **Responsibility:** Defines the raw, internal data structures that map 1:1 to the XDC XML schema.
  - **Details:** These structs are considered an implementation detail and are not exposed publicly. They are heavily annotated with `#[serde(...)]` attributes to guide `quick-xml`. Their goal is to capture the XML data as-is, including `String` representations of enums, hex values, etc.
- **`src/resolver.rs` (Business Logic)**
  - **Responsibility:** The "brains" of the crate. It converts the "dumb" `model` structs into the "smart" public `types` structs.
  - **Details:** This module contains all the business logic for parsing string values into enums, converting hex strings into `Vec<u8>`, validating data, and resolving data types. It bridges the gap between the raw XML and the ergonomic public API, handling all error conditions gracefully.
- **`src/types.rs` (Public API)**
  - **Responsibility:** Defines the public, ergonomic data structures that consumers of this crate will interact with.
  - **Details:** These structs are clean, well-documented, and use rich types (e.g., enums, `u16`) instead of strings. Where possible and logical, they use types directly from the `powerlink-rs` crate (like `powerlink_rs::types::AccessType`) to ensure interoperability.
- **`src/error.rs`**
  - **Responsibility:** Defines the crate's custom `XdcError` enum.
  - **Details:** Provides detailed error information, distinguishing between XML parsing errors (from `quick-xml`) and data resolution errors (e.g., "Invalid AccessType string").
- **`src/lib.rs`**
  - **Responsibility:** The main crate library entry point.
  - **Details:** Re-exports the public API from `src/types.rs` and the main `parse_xdc` function from `src/parser.rs`.
- **`src/builder.rs`**
  - **Responsibility (Future):** Will provide a "builder" API for programmatically constructing a new `XdcFile` struct from scratch.
  - **Details:** This is planned for a future phase and will be essential for tools that need to create or modify XDC files, not just read them.

## XDC Specification Coverage

This table tracks the crate's implementation status against the main features of the EPSG DS 311 specification.

| Feature / Element | XSD Definition | Status | Notes |
| :--- | :--- | :--- | :--- |
| **ProfileHeader** | `ProfileHeader_DataType` | 🟡 **In Progress** | Core ISO 15745 fields are being added. |
| **ProfileBody** | `ProfileBody_DataType` | 🟡 **InProgress** | |
| ➡️ **ObjectList** | `ag_Powerlink_ObjectList` | 🟡 **InProgress** | Parsing objects, but attribute support is partial. |
| ➡️ **Object** | `ag_Powerlink_Object` | 🔴 **Partial** | Missing `name`, `accessType`, `PDOmapping`, etc. |
| ➡️ **SubObject** | `ag_Powerlink_Object` | 🔴 **Partial** | Missing `name`, `accessType`, `PDOmapping`, etc. |
| ➡️ **NetworkManagement** | `ProfileBody_CommunicationNetwork_Powerlink.xsd` | 🔴 **Not Started** | Entire section is currently un-parsed. |
| ➡️ **GeneralFeatures** | `ct_GeneralFeatures` | 🔴 **Not Started** | |
| ➡️ **MNFeatures** | `ct_MNFeatures` | 🔴 **Not Started** | |
| ➡️ **CNFeatures** | `ct_CNFeatures` | 🔴 **Not Started** | |
| ➡️ **Diagnostic** | `ct_Diagnostic` | 🔴 **Not Started** | |

## Roadmap

### Phase 1: Core Model & API

- **Focus:** Establish the 3-stage architecture and parse the most critical 80% of XDC data: the Object Dictionary.
- **Key Features:**
  - Complete `serde` models for `ProfileHeader`.
  - Complete `serde` models for `Object` and `SubObject`, including all attributes (`name`, `accessType`, `PDOmapping`, `objFlags`, etc.).
- **Success Metric:** The crate can successfully parse a real-world XDC file and provide full, typed access to its entire Object Dictionary.
- **Status:** 🟢 **In Progress**

### Phase 2: Full Specification Compliance

- **Focus:** Implement parsing for the remaining sections of the XDC schema, primarily `NetworkManagement`.
- **Key Features:**
  - Add `serde` models for `NetworkManagement`, `GeneralFeatures`, `MNFeatures`, `CNFeatures`, and `Diagnostic`.
  - Add public `types` for the `NetworkManagement` data.
  - Implement `resolver.rs` logic to map and validate this data.
  - Robustly handle all data types and complex parameter definitions.
- **Success Metric:** The crate can parse 100% of the elements and attributes defined in the EPSG DS 311 XSDs.
- **Status:** 🔴 **Not Started**

### Phase 3: Validation & Ergonomics

- **Focus:** Move beyond simple parsing to provide data validation and creation (builder) tools.
- **Key Features:**
  - Implement the `builder.rs` API for programmatically creating new `XdcFile` structs.
  - Add a `validate()` method to `XdcFile` that checks for common configuration errors (e.g., invalid PDO mappings, missing mandatory objects).
  - Implement `quick-xml` serialization to write an `XdcFile` struct back to an XML string.
- **Success Metric:** A user can create a valid XDC file from scratch, serialize it to XML, parse it back, and get an identical struct.
- **Status:** 🔴 **Not Started**
