# powerlink-rs

Robust, reliable, and platform-independent Rust implementation of the Ethernet Powerlink protocol.

**Work in progress**.

## Coverage

This section tracks the implementation status against the **EPSG 301 V1.5.1 Communication Profile Specification**.

- **EPSG 301 (Communication Profile Specification):**
  - **Chapter 4 (Data Link Layer): 95%**
    - `[x]` 4.6 Frame Structures (SoC, PReq, PRes, SoA, ASnd)
    - `[x]` 4.2.4.5 CN Cycle State Machine (DLL_CS)
    - `[x]` 4.2.4.6 MN Cycle State Machine (DLL_MS)
    - `[x]` 4.7 DLL Error Handling & Counters (CN/MN)
    - `[x]` 4.2.5 Recognizing Active Nodes (IdentRequest/Response)
    - `[~]` 4.2.4.1.1.1 Multiplexed Timeslots (Basic support implemented; complex scheduling not yet optimized)
  - **Chapter 5 (Network/Transport Layer): 100%**
    - `[x]` 5.1 IP Addressing (Logic assumes 192.168.100.x subnet)
    - `[x]` 5.2 POWERLINK compliant UDP/IP format (for SDO)
    - `[x]` SDO over UDP/IP HAL (`NetworkInterface` trait)
    - `[x]` Core SDO/UDP serialization (`sdo/udp.rs`)
    - `[x]` Integration of `receive_udp` into `Node::run_cycle`
    - `[x]` 5.1.3 Address Resolution (ARP) (Passive ARP cache populated from IdentResponse. Active ARP client not implemented.)
    - `[x]` 5.1.4 Hostname (OD `0x1F9A` is set via `NMTNetHostNameSet`)
  - **Chapter 6 (Application Layer): 85%**
    - `[x]` 6.1 Basic Data Types & Encoding (in `types.rs`, `od/value.rs`)
    - `[x]` 6.2 Object Dictionary Structure (in `od/` module)
    - `[x]` 6.3.2 Service Data Objects (SDO) via ASnd (Mandatory commands: Read/WriteByIndex)
    - `[x]` 6.3.2 Service Data Objects (SDO) via UDP/IP (Mandatory commands: Read/WriteByIndex)
    - `[x]` 6.4 Process Data Objects (PDO) (Mapping, validation, error handling)
    - `[x]` 6.5 Error Signaling (EN/EA/ER/EC flags, StatusResponse)
    - `[x]` 6.3.3 SDO Embedded in PDO
    - `[~]` Optional SDO Commands (`WriteAllByIndex`, `WriteMultipleParamByIndex`, etc.) are *not* implemented in the core, but are supported via the `SdoCommandHandler` trait for applications to implement.
    - `[~]` 6.6 Program Download (PDL) (Considered an application-level task. The crate provides the SDO mechanism (segmented `WriteByIndex` to 0x1F50) for the application to use.)
    - `[ ]` 6.7 Configuration Management (CFM) (MN logic to send configuration via SDO is missing. The `powerlink-rs-xdc` crate will provide the parsing logic for this.)
  - **Chapter 7 (Network Management): 100%**
    - `[x]` 7.1 NMT State Machines (Common, MN, CN)
    - `[x]` 7.3.1 NMT State Command Services (StartNode, StopNode, Resets)
    - `[x]` 7.3.3 NMT Response Services (IdentResponse, StatusResponse)
    - `[x]` 7.3.6 Request NMT Services by a CN (ASnd NMTRequest)
    - `[x]` 7.3.5 NMT Guard Services (SoC/PRes timeouts, Consumer Heartbeat)
    - `[x]` 7.4 MN Boot-up Sequence (Full validation: 7.4.2.2.1.1 - 7.4.2.2.1.3)
    - `[x]` 7.3.4 NMT Info Services (MN publishing `NMTPublish...` frames)
    - `[x]` 7.3.2 NMT Managing Command Services (e.g., `NMTNetHostNameSet`)
  - **Chapter 8 (Diagnostics): 50%**
    - `[x]` 8.1 Diagnostic OD Entries (`0x1101`, `0x1102`) (Counters are incremented)
    - `[~]` `powerlink-rs-monitor` (In-process web monitor, in development)
  - **Chapter 9 (Routing): 0%**
    - `[ ]` 9.1 Routing Type 1
    - `[ ]` 9.2 Routing Type 2
  - **Chapter 10 (Indicators): Not applicable**
    - Hardware-specific, outside core library scope.
- **EPSG 302 (Extensions): 0%**
  - `[ ]` EPSG 302-A (High Availability)
  - `[ ]` EPSG 302-B (Multiple ASnd)
  - `[ ]` EPSG 302-C (PollResponse Chaining)
  - `[ ]` EPSG 302-D (Multiple PReq/PRes)
  - `[ ]` EPSG 302-E (Dynamic Node Allocation)

## Testing

Some integration tests requiring access to the network layer. `#[ignore]` is used with these tests as they require root privileges, so these are ignored by default. They can still be ran, for example with Linux, by using `sudo` and using the full path to the cargo executable (example: `sudo -E /home/<user_name>/.cargo/bin/cargo test --package powerlink-rs-linux --test loopback_test -- test_cn_responds_to_preq_on_loopback --exact --show-output --ignored`). Another way to handle this is by running the tests within Docker containers.

To aid in debugging these complex integration tests, a dedicated `powerlink-rs-monitor` crate is planned. This tool will provide a web-based GUI to visualize the NMT state, error counters, and other diagnostic data in real-time. It is designed to run alongside the node (either in-process for development or as a separate diagnostic node) and will be the primary tool for observing test behavior, supplementing raw `.pcap` logs.

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
  - SDO Philosophy: The core crate implements all *mandatory* SDO commands (e.g., `ReadByIndex`, `WriteByIndex`). *Optional* commands (e.g., `WriteAllByIndex`, `WriteMultipleParamByIndex`) are *not* implemented in the core, but are supported via the `SdoCommandHandler` trait for applications to implement.
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
    - MN Cycle Orchestration: Refine `ManagingNode::tick` to precisely follow the isochronous and asynchronous phases based on OD timings.
    - MN Boot-Up Sequence: Implement the detailed boot-up logic for identifying, checking, and commanding state transitions for CNs (Chapter 7.4).
    - CN Response Logic: Ensure `ControlledNode` reacts correctly to MN frames (SoC, PReq, SoA) according to its NMT/DLL state.
    - DLL Error Handling Integration: Ensure DLL error counters correctly trigger the specified NMT state changes.
  - Success Metric: A simulated MN/CN pair can successfully transition to the NMT_CS_OPERATIONAL state and maintain a stable POWERLINK cycle.
  - Status: **Completed**.
- Phase 7: Debugging and Monitoring (`powerlink-rs-monitor`):
  - Focus: Implement a dedicated `powerlink-rs-monitor` crate. This tool will provide a web-based GUI for real-time diagnostics.
  - Key features:
    - In-Process (Default): Runs as a non-real-time (NRT) thread in the same application as the node, communicating via an RT-safe channel (e.g., `crossbeam-channel`). This provides deep, internal state visibility for development and debugging without impacting network performance.
    - Out-of-Process (Standard): Runs as a separate, standard-compliant Diagnostic Node (e.g., Node 253) that polls the MN's diagnostic OD entries (0x1F8E, 0x1101, etc.) via SDO, allowing it to monitor any compliant MN on the network.
  - Status: **In development**.
- Phase 8: Integration Testing and Validation:
  - Focus: Creating robust integration tests for the full MN/CN communication cycle.
  - Key Features:
    - Develop Docker-based tests for the full boot-up sequence.
    - Test PDO data exchange in `NMT_OPERATIONAL` state.
    - Test NMT command handling (e.g., `StopNode`, `ResetNode`).
    - Test SDO (ASnd) read/write of mandatory commands (`ReadByIndex`, `WriteByIndex`).
  - Success Metric: All integration tests pass, demonstrating a stable and conformant basic network operation.
  - Status: **In development**. The immediate focus is on expanding the Docker-based integration tests to validate the full boot-up sequence, PDO exchange, and error handling.
- Future (post DS-301):
  - Microcontroller Support: Implement a `no_std` I/O module targeting a specific embedded MAC/PHY driver using the traits defined in Phase 4.
  - Configuration Files: Implement parsers for the `XML` Device Description (`XDD`) and `XML` Device Configuration (`XDC`) files (defined by EPSG DS-311). This is supported by the `powerlink-rs-xdc` crate.
  - Extensions (DS-302 Series): Add support for non-mandatory features defined in extension specifications, such as:
    - High Availability (EPSG DS-302-A).
    - Multiple-ASnd (EPSG DS-302-B).
    - PollResponse Chaining (EPSG DS-302-C).
    - Multiple PReq/PRes (EPSG DS-302-D).
    - Dynamic Node Allocation (EPSG DS-302-E).
- Hopefully one day:
  - **Conformance Testing**: Development effort should eventually include test cases inspired by the requirements documented in the EPSG DS-310 Conformance Test Specification.  

## Licensing

This project is licensed under the **Apache License, Version 2.0** (the "License"). You may not use this project except in compliance with the License.

A copy of the License is provided: [link to copy of the license](LICENSE).

## IMPORTANT DISCLAIMER REGARDING THE POWERLINK STANDARD

**NOTICE:** While the source code for this library is provided under the permissive Apache 2.0 license, the underlying Ethernet POWERLINK protocol is implemented based on specifications provided by B&R Industrial Automation GmbH (successor to the EPSG) that carry explicit waivers of liability and patent warnings.

**ALL USERS ARE REQUIRED TO READ AND UNDERSTAND THE FULL DISCLAIMER BEFORE USING THIS CODE.**

-> **[View Full Disclaimer and Patent Notice (`DISCLAIMER.md`)](DISCLAIMER.md).
