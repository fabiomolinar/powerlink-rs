# Ethernet Powerlink Standard

The Ethernet Powerlink standard can be found at [B&R Automation downloads page](https://www.br-automation.com/en/technologies/powerlink/service-downloads/).

The specification is divided into the following files:

- **POWERLINK Communication Profile Specification**
  - EPSG 301 V-1-5-1 DS.pdf
- **Extension specifications**
  - EPSG 302-A V-1-1-1 DS.pdf
  - EPSG 302-B V-1-1-1 DS.pdf
  - EPSG 302-C V-1-1-1 DS.pdf
  - EPSG 302-D V-1-1-1 DS.pdf
  - EPSG 302-E V-1-2-1 DS.pdf
  - EPSG 302-F V-1-0-1 DS.pdf
- **XML Device Description**
  - EPSG 311 V-1-2-1 DS.pdf
  - XML Device Description – Implementation Guidelines V-1-0-2.pdf
- **Further technical specification**
  - POWERLINK_Implementation_Directive_for_CiA402_EPSG_V-0-0-4.pdf
  - EPSG_XML_header_for_firmware_files_V-1-0-0.pdf

An additional companion specification exists related to OPC-UA:

- **OPCUA POWERLINK Companion Specification**
  - OPCUA POWERLINK Companion Specification RELEASE 1.0.pdf

## Highlights

> What follows are notes taken from the specification itself. All images found in this section were sourced from the original specification.

POWERLINK provides mechanisms to achieve the following:

1. Transmit time-critical data in precise isochronous cycles. Data exchange is based on a publish/subscribe relationship. Isochronous data communication can be used for exchanging position data of motion applications of the automation industry.
2. Synchronise networked nodes with high accuracy.
3. Transmit less time-critical data asynchronously on request. Asynchronous data communication can be used to transfer IP-based protocols like TCP or UDP and higher layer protocols such as HTTP, FTP,…

POWERLINK manages the network traffic in a way that there are dedicated time-slots for isochronous and asynchronous data. The mechanism is called Slot Communication Network Management (SCNM). SCNM is managed by one particular networked device – the Managing Node (MN) – which includes the MN functionality. All other nodes are called Controlled Nodes (CN).

POWERLINK is based on the ISO/OSI layer model and supports Client/Server and Producer/Consumer communications relationships.

The POWERLINK communication profile is based on CANopen communication profiles DS301 and DS302.

The advantages of POWERLINK result from protecting the POWERLINK RTE network segment from regular office and factory networks. POWERLINK provides a private Class-C IP segment solution with fixed IP addresses.

**Reference model**:
[Reference Model](reference_model.png)

### Application layer

The Application Layer comprises a concept to configure and communicate real-time-data as well as the mechanisms for synchronisation between devices. The functionality the application layer offers to an application is logically divided over different service objects (see SDO) in the application layer. A service object offers a specific functionality and all related services.
