# Architecture Overview

This document describes the foundational architectural decisions of the `powerlink-rs` project, focusing on code structure, portability, and modularity, which enable its goal of being a robust, reliable, and platform-independent Rust implementation of the Ethernet POWERLINK protocol.

## Crate Structure and Modular Design (The Workspace)

The project utilizes a Rust **workspace** to achieve clear separation between the core protocol logic and platform-specific network communication.

1.  **The Core Crate (`powerlink-rs`):** This is defined as the **"Platform-agnostic core logic for Ethernet POWERLINK Rust implementation"**. It contains the fundamental protocol state machines and data structures necessary to implement the standard (e.g., NMT cycle logic, frame parsing/generation). It is set as the default member of the workspace.
2.  **I/O Driver Crates (`powerlink-io-*`):** These separate crates handle the low-level, raw Ethernet input/output (I/O) for different operating systems and embedded environments. Current planned drivers include `powerlink-io-linux`, `powerlink-io-windows`, and `powerlink-io-embedded`.

## Resource Management and Portability (The `no_std` Core)

To ensure the implementation can run across diverse targets, including embedded systems, the architecture prioritizes minimal reliance on a full operating system environment.

- **Platform Independence:** The overall project aims for a platform-independent implementation, supporting environments like Windows, Linux, macOS, and embedded targets.
- **`no_std` Compatibility:** The core logic within the `powerlink-rs` crate is designed to support `no_std` environments (environments without the Rust standard library).
- **Feature Flag Strategy:** Cross-platform compilation is managed via feature flags. The platform-specific I/O crates (like `powerlink-io-windows`) explicitly enable the **`std` feature** of the core `powerlink-rs` crate when standard library functionality (such as OS sockets) is required for those platforms..
- **alloc:** At least for now, we are relying on alloc for dynamic memory allocation. This may change on the future.

## Hardware Abstraction Layer (HAL)

To decouple the core protocol logic from the physical network interface, a Hardware Abstraction Layer (HAL) approach is mandated.

- **HAL Trait Definition:** The core `powerlink-rs` crate defines a Rust trait for low-level I/O, abstracting functions such as `send_raw_frame` and `receive_raw_frame`.
- **Platform Implementation:** The platform-specific crates (e.g., `powerlink-io-windows`) are responsible for implementing this **core HAL**, utilizing platform-native APIs (such as raw sockets or specialized bindings like WinPcap/Npcap on Windows) to handle raw Ethernet packet interaction.

## Internal Protocol Layering and Code Modules

The core implementation mirrors the functional layers defined in the EPSG DS 301 Communication Profile Specification. The mapping of protocol concepts to Rust modules ensures logical encapsulation of responsibility:

| POWERLINK Concept | Role in Architecture |
| :--- | :--- |
| **Data Link Layer (DLL) / Frames** | Core modules handle the parsing and serialization of basic frames (e.g., SoC, SoA, PReq, PRes). Phase 1 focuses heavily on this layer. |
| **Object Dictionary (OD)** | Module responsible for defining the structure and handling of data objects accessible over POWERLINK communication, using Index and Sub-Index addressing. |
| **Network Management (NMT)** | Modules implementing the Network Management state machines (MN and CN states, e.g., `NMT_CS_NOT_ACTIVE` to `NMT_CS_OPERATIONAL`) and handling configuration objects. |
| **Service Data Objects (SDO)** | Modules implementing non-real-time data exchange (client/server model). SDO services are implemented using sequenced commands over asynchronous frames (ASnd) or UDP/IP. |
| **Process Data Objects (PDO)** | Modules handling real-time, cyclic data exchange (Producer/Consumer model) carried within PReq and PRes frames. |

## Naming Conventions

This crate tries to keep the Rust standard when creating names. However, where appropriate, it should document what are the names defined by the specification.
