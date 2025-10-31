# powerlink-rs

Robust, reliable, and platform-independent Rust implementation of the Ethernet Powerlink protocol.

**Work in progress**.

## Coverage

- **EPSG 301 (Communication Profile Specification):**
  - **Chapter 4 (Data Link Layer): 90%**
    - [x] 4.6 Frame Structures (SoC, PReq, PRes, SoA, ASnd)
    - [x] 4.2.4 Cycle State Machines (MN and CN)
    - [x] 4.7 DLL Error Handling
    - [x] Other DLL concepts
  - **Chapter 5 (Network/Transport Layer): 0%**
  - **Chapter 6 (Application Layer): 90%**
    - [x] 6.1 Basic Data Types (`NetTime`, `RelativeTime`)
    - [x] 6.2 Object Dictionary Structure
    - [x] 6.3 Service Data Objects (SDO) via ASnd
    - [x] 6.4 Process Data Objects (PDO) with mapping, validation, and error handling
    - [x] 6.5 Error Signaling
  - **Chapter 7 (NMT): 80%**
    - [x] Basic NMT data structures (`NmtState`)
    - [x] 7.1 NMT State Machines
    - [ ] 7.3 NMT Services (partially implemented for MN boot-up)
    - [x] 7.4 MN Boot-up (initial logic in place)
  - **Chapter 8 (Diagnostics): 0%**
  - **Chapter 9 (Routing): 0%**
- **EPSG 302-A (High Availability)**: 0% (for the future)
- **EPSG 302-B (Multiple ASnd)**: 0% (for the future)
- **EPSG 302-C (PollResponse Chaining)**: 0% (for the future)
- **EPSG 302-D (Multiple PReq/PRes)**: 0% (for the future)
- **EPSG 302-E (Dynamic Node Allocation)**: 0% (for the future)
- **EPSG 311 (Device Description)**: 0% (for the future)

## Testing

Some integration tests requiring access to the network layer. `#[ignore]` is used with these tests as they require root privileges, so these are ignored by default. They can still be ran, for example with Linux, by using `sudo` and using the full path to the cargo executable (example: `sudo -E /home/<user_name>/.cargo/bin/cargo test --package powerlink-rs-linux --test loopback_test -- test_cn_responds_to_preq_on_loopback --exact --show-output --ignored`).

## Roadmap

- Phase 1: Foundation and Data Link Layer (DLL) Packet Handling:
  - Focus: Implementing the lowest-level structures and serialization/deserialization logic.
  - Key Features (DS-301): Definition of basic types (Node ID, data sizes), frame construction/parsing (Ethernet II, POWERLINK basic frame format), and handling of fundamental control frames (SoC, SoA) and data frames (PReq, PRes).
  - Success Metric: The crate can accurately generate and parse raw byte arrays corresponding to basic POWERLINK frames.
  - Status: **Completed**.
- Phase 2: Object Dictionary and Basic Network Management (NMT):
  - Focus: Core configuration logic and device identification.
  - Key Features (DS-301): Implementation of the Object Dictionary (OD) structure (Index and Sub-Index usage), defining mandatory NMT objects (e.g., identity object 1018h, NMT features 1F82h, EPL version 1F83h), and implementing the fundamental NMT State Machines (Common, MN, and CN states, e.g., NMT_CS_NOT_ACTIVE to NMT_CS_OPERATIONAL).
  - Success Metric: The device can maintain internal NMT state correctly and respond to simulated NMT state commands.
  - Status: **Completed**.
- Phase 3: Service Data Object (SDO) Communication:
  - Focus: Reliable, asynchronous configuration and diagnostic access over ASnd frames.
  - Key Features (DS-301): Implementation of the SDO Command Layer Protocol (e.g., Read/Write by Index requests), the SDO Sequence Layer (for reliability), and integration for transfer via the mandatory ASnd frame (Method 2, signaled by NMT_FeatureFlags_U32 Bit 2).
  - Success Metric: Successful simulated read/write transactions (SDO client and server) to the mock Object Dictionary.
  - Status: **Completed**.
- Phase 4: Platform Abstraction and Initial I/O Layer:
  - Focus: Enabling cross-platform usage and testing.
  - Key Features: Define the core Rust Trait for low-level I/O (send_raw_frame, receive_raw_frame). Implement the initial platform-specific driver modules for Linux/Windows (using sockets/raw interfaces). Optionally, support the use of SDO via UDP/IP (Method 1, signaled by NMT_FeatureFlags_U32 Bit 1).
  - Success Metric: The core protocol logic can run and exchange actual Ethernet frames on a standard operating system using a loopback or virtual network environment.
  - Status: **Completed**.
- Phase 5: Real-Time Data Handling (PDO):
  - Focus: Implementing the core real-time communication mechanism.
  - Key Features (DS-301): Implementation of PDO Mapping structures (Transmit PDOs 1800h-1AFFh, Receive PDOs 1400h-16FFh), and the logic to insert/extract process data into/from PReq and PRes frames based on the cycle timing.
  - Success Metric: The library can correctly map application variables to PDO payloads during simulated cyclic data exchange.
  - Status: **Completed**.
- Phase 6: Core NMT Cycle Logic (MN/CN Implementation):
  - Focus: Implementing the roles required to run an entire POWERLINK network.
  - Key Features (DS-301):
    - **MN Cycle Orchestration**: Refine `ManagingNode::tick` to precisely follow the isochronous and asynchronous phases based on OD timings.
    - **MN Boot-Up Sequence**: Implement the detailed boot-up logic for identifying, checking, and commanding state transitions for CNs (Chapter 7.4).
    - **CN Response Logic**: Ensure `ControlledNode` reacts correctly to MN frames (SoC, PReq, SoA) according to its NMT/DLL state.
    - **DLL Error Handling Integration**: Ensure DLL error counters correctly trigger the specified NMT state changes.
  - Success Metric: A simulated MN/CN pair can successfully transition to the NMT_CS_OPERATIONAL state and maintain a stable POWERLINK cycle.
  - Status: **In development**. Core MN scheduling and CN response logic is implemented and undergoing refinement.
- Phase 7: Integration Testing and Validation:
  - Focus: Creating robust integration tests for the full MN/CN communication cycle.
  - Key Features:
    - Develop Docker-based tests for the full boot-up sequence.
    - Test PDO data exchange in `NMT_OPERATIONAL` state.
    - Test NMT command handling (e.g., `StopNode`, `ResetNode`).
    - Test DLL error handling scenarios (e.g., PRes timeouts).
  - Success Metric: All integration tests pass, demonstrating a stable and conformant basic network operation.
  - Status: **In development**. Initial Docker-based integration tests for the boot-up sequence and SDO communication are in place.
- Future (post DS-301):
  - Microcontroller Support: Implement a `no_std` I/O module targeting a specific embedded MAC/PHY driver using the traits defined in Phase 4.
  - Configuration Files: Implement parsers for the `XML` Device Description (`XDD`) and `XML` Device Configuration (`XDC`) files (defined by EPSG DS-311).
  - Extensions (DS-302 Series): Add support for non-mandatory features defined in extension specifications, such as:
    - High Availability (EPSG DS-302-A).
    - Multiple-ASnd (EPSG DS-302-B).
    - PollResponse Chaining (EPSG DS-302-C).
    - Multiple PReq/PRes (EPSG DS-302-D).
    - Dynamic Node Allocation (EPSG DS-302-E).
- Hopefully one day:
  - Conformance Testing: Development effort should eventually include test cases inspired by the requirements documented in the EPSG DS-310 Conformance Test Specification.
  - Debugging and monitoring tools.

## Licensing

This project is licensed under the **Apache License, Version 2.0** (the "License"). You may not use this project except in compliance with the License.

A copy of the License is provided: [link to copy of the license](LICENSE).

## IMPORTANT DISCLAIMER REGARDING THE POWERLINK STANDARD

**NOTICE:** While the source code for this library is provided under the permissive Apache 2.0 license, the underlying Ethernet POWERLINK protocol is implemented based on specifications provided by B&R Industrial Automation GmbH (successor to the EPSG) that carry explicit waivers of liability and patent warnings.

**ALL USERS ARE REQUIRED TO READ AND UNDERSTAND THE FULL DISCLAIMER BEFORE USING THIS CODE.**

-> **[View Full Disclaimer and Patent Notice (`DISCLAIMER.md`)](DISCLAIMER.md)**
