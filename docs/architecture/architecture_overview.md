# Architecture Overview

This document describes the foundational architectural decisions of the `powerlink-rs` project, focusing on code structure, portability, and modularity, which enable its goal of being a robust, reliable, and platform-independent Rust implementation of the Ethernet POWERLINK protocol.

## Crate Structure and Modular Design (The Workspace)

The project utilizes a Rust **workspace** to achieve clear separation between the core protocol logic and platform-specific network communication.

1. **The Core Crate (`powerlink-rs`):** This is defined as the **"Platform-agnostic core logic for Ethernet POWERLINK Rust implementation"**. It contains the fundamental protocol state machines and data structures necessary to implement the standard (e.g., NMT cycle logic, frame parsing/generation). It is set as the default member of the workspace.
2. **I/O Driver Crates (`powerlink-rs-*`):** These separate crates handle the low-level, raw Ethernet input/output (I/O) for different operating systems and embedded environments. Current planned drivers include `powerlink-rs-linux`, `powerlink-rs-windows`, and `powerlink-rs-embedded`.

## Resource Management and Portability (The `no_std` Core)

To ensure the implementation can run across diverse targets, including embedded systems, the architecture prioritizes minimal reliance on a full operating system environment.

- **Platform Independence:** The overall project aims for a platform-independent implementation, supporting environments like Windows, Linux, macOS, and embedded targets.
- **`no_std` Compatibility:** The core logic within the `powerlink-rs` crate is designed to support `no_std` environments (environments without the Rust standard library).
- **Feature Flag Strategy:** Cross-platform compilation is managed via feature flags. The platform-specific I/O crates (like `powerlink-rs-windows`) explicitly enable the **`std` feature** of the core `powerlink-rs` crate when standard library functionality (such as OS sockets) is required for those platforms..
- **alloc:** At least for now, we are relying on alloc for dynamic memory allocation. This may change on the future.

## Crate independence

To the extent possible, this crate will try to avoid adding other crates as dependencies.

## Hardware Abstraction Layer (HAL)

To decouple the core protocol logic from the physical network interface, a Hardware Abstraction Layer (HAL) approach is mandated.

- **HAL Trait Definition:** The core `powerlink-rs` crate defines a Rust trait for low-level I/O, abstracting functions such as `send_frame` and `receive_frame`.
- **Platform Implementation:** The platform-specific crates (e.g., `powerlink-rs-windows`) are responsible for implementing this **core HAL**, utilizing platform-native APIs (such as raw sockets or specialized bindings like WinPcap/Npcap on Windows) to handle raw Ethernet packet interaction.

## Internal Protocol Layering and Code Modules

The core implementation mirrors the functional layers defined in the EPSG DS 301 Communication Profile Specification. The mapping of protocol concepts to Rust modules ensures logical encapsulation of responsibility:

| POWERLINK Concept | Role in Architecture |
| :--- | :--- |
| **Data Link Layer (DLL) / Frames** | Core modules handle the parsing and serialization of basic frames (e.g., SoC, SoA, PReq, PRes). Phase 1 focuses heavily on this layer. |
| **Object Dictionary (OD)** | Module responsible for defining the structure and handling of data objects accessible over POWERLINK communication, using Index and Sub-Index addressing. |
| **Network Management (NMT)** | Modules implementing the Network Management state machines (MN and CN states, e.g., `NMT_CS_NOT_ACTIVE` to `NMT_CS_OPERATIONAL`) and handling configuration objects. |
| **Service Data Objects (SDO)** | Modules implementing non-real-time data exchange (client/server model). **The core crate implements *mandatory* SDO commands (e.g., `ReadByIndex`, `WriteByIndex`) and the `SdoClientManager` for segmented transfers**. *Optional* SDO commands are handled by the `SdoCommandHandler` trait. High-level features like **Program Download (PDL)** are considered application-level tasks; the crate provides the SDO mechanism (e.g., `mn.write_object(0x1F50, ...)`), while the application provides the firmware data and the "intent" to download. |
| **Process Data Objects (PDO)** | Modules handling real-time, cyclic data exchange (Producer/Consumer model) carried within PReq and PRes frames. |

## Diagnostics and Monitoring (`powerlink-rs-monitor`)

Debugging a real-time network protocol by observing application logs is inherently difficult and inefficient. To solve this, a dedicated `powerlink-rs-monitor` crate is planned to provide a flexible, graphical monitoring tool (e.g., a web-based GUI).

A critical requirement is that the monitoring tool **must never interfere with or block the real-time POWERLINK cycle**.

To achieve this, the `powerlink-rs-monitor` crate will be designed to operate in two distinct modes:

### Approach 1: In-Process (Default)

This mode is designed for high-performance, deep diagnostics during development and testing.

- **"How":** The monitor runs as a **non-real-time (NRT) thread** within the same application as the `ManagingNode` or `ControlledNode`.
  - **Real-Time (RT) Thread:** Runs the core POWERLINK `Node` logic.
  - **Non-Real-Time (NRT) Thread:** Runs a `tokio` async runtime, a web server (e.g., `axum`), and a WebSocket endpoint for the GUI.
  - **The "Plug":** The two threads communicate via a **bounded, real-time-safe channel** (e.g., `crossbeam-channel::bounded(1)`).
- **"Why":** At the end of each cycle, the RT thread copies its internal state (NMT state, error counters, scheduler status, etc.) into a snapshot struct and uses **`sender.try_send(snapshot)`** to send it to the NRT thread.
  - If the web server is busy, the channel is full, `try_send` fails immediately, and the snapshot is dropped.
  - This **guarantees the real-time loop is never blocked** by the web server.
  - It also provides complete, internal visibility of the node's state with zero impact on network bandwidth.

### Approach 2: Standard-Compliant (Out-of-Process)

This mode acts as a standard-compliant, external diagnostic tool, as defined by the EPSG specification.

- **"How":** The `powerlink-rs-monitor` application runs as a completely separate process. It initializes its own `ControlledNode` stack and joins the network as a **Diagnostic Device (Node ID 253)**.
- **"Why":** It acts as a standard SDO client, polling the `ManagingNode` (Node 240) for data using standard SDO `ReadByIndex` requests (over ASnd or UDP).
  - It polls standard diagnostic objects like `NMT_MNNodeCurrState_AU8 (0x1F8Eh)` to get all CN states.
  - It polls diagnostic counters like `DIA_NMTTelegrCount_REC (0x1101h)` and `DIA_ERRStatistics_REC (0x1102h)`.
- **Advantages:** This approach is fully interoperable and can monitor any compliant MN (not just one built with `powerlink-rs`). It also serves as an excellent integration test for our SDO client/server stack.
- **Disadvantages:** It creates additional network load in the asynchronous phase and can only view data explicitly exposed in the MN's Object Dictionary.

## Naming Conventions

This crate tries to keep the Rust standard when creating names. However, where appropriate, it should document what are the names defined by the specification.
