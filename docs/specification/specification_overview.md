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

> What follows are extracts taken from the specification itself. All images found in this section come from the original specification.

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

### Application layer (2.1.1)

The Application Layer comprises a concept to configure and communicate real-time-data as well as the mechanisms for synchronisation between devices. The functionality the application layer offers to an application is logically divided over different service objects (see SDO) in the application layer. A service object offers a specific functionality and all related services.

Service primitives are the means by which the application and the application layer interact. There are four different primitives:

- a *request* is issued by the application to the application layer to request a service
- an *indication* is issued by the application layer to the application to report an internal event detected by the application layer or indicate that a service is requested
- a *response* is issued by the application to the application layer to respond to a previous received indication
- a *confirmation* is issued by the application layer to the application to report the result of a previously issued request.

A service type defines the primitives that are exchanged between the application layer and the co-operating applications for a particular service of a service object.

- A *Local Service* involves only the local service object. The application issues a request to its local service object that executes the requested service without communicating with (a) peer service object(s).
- An *Unconfirmed Service* involves one or more peer service objects. The application issues a request to its local service object. This request is transferred to the peer service object(s) that each pass it to their application as an indication. The result is not confirmed back.
- A *Confirmed Service* can involve only one peer service object. The application issues a request to its local service object. This request is transferred to the peer service object that passes it to the other application as an indication. The other application issues a response that is transferred to the originating service object that passes it as a confirmation to the requesting application.
- A *Provider Initiated Service* involves only the local service object. The service object (being the service provider) detects an event not solicited by a requested service. This event is then indicated to the application.

Unconfirmed and confirmed services are collectively called *Remote Services*.

### Device Model (2.2)

A device is structured as follows:

- *Communication* – This function unit provides the communication objects and the appropriate functionality to transport data items via the underlying network structure.
- *Object Dictionary* – The Object Dictionary is a collection of all the data items that have an influence on the behaviour of the application objects, the communication objects and the state machine used on this device.
- *Application* – The application comprises the functionality of the device with respect to the interaction with the process environment.

Thus the Object Dictionary serves as an interface between the communication and the application. The complete description of a device’s application with respect to the data items in the Object Dictionary is called the device profile.

### The Object Dictionary (2.2.2)

The most important part of a device profile is the Object Dictionary. The Object Dictionary is essentially a grouping of objects accessible via the network in an ordered, pre-defined fashion. Each object within the dictionary is addressed using a **16-bit index**. The Object Dictionary may contain **a maximum of 65536 entries** which are addressed through a 16-bit index.

[Object Dictionary](object_dictionary.png)

- The Static Data Types at indices *0001h through 001Fh* contain type definitions for standard data types like BOOLEAN, INTEGER, floating point, string, etc.
- Manufacturer Specific Complex Data Types at indices 0040h through 005Fh are structures composed of standard data types but are specific to a particular device.
- Device Profiles may define additional data types specific to their device type. The static data types defined by the device profile are listed at indices 0060h - 007Fh, the complex data types at indices 0080h - 009Fh.
- A device may optionally provide the structure of the supported complex data types (indices 0020h - 005Fh and 0080h - 009Fh) at read access to the corresponding index. Sub-index 0 provides the number of entries at this index, and the following sub-indices contain the data type encoded as UNSIGNED16.
- POWERLINK Specific Static Data Types shall be described at indices 0400h – 041Fh. These entries are included for reference only; they cannot be read or written.
- POWERLINK Specific Complex Data Types shall be described at indices 0420h – 04FFh
- The Communication Profile Area at indices 1000h through 1FFFh contains the communication specific parameters for the POWERLINK network. These entries are common to all devices.
- The standardised device profile area at indices 6000h through 9FFFh contains all data objects common to a class of devices that can be read or written via the network. The device profiles may use entries from 6000h to 9FFFh to describe the device parameters and the device functionality. Within this range up to **8 different devices** can be described. In such a case the devices are denominated *Multiple Device Modules*.
  - Multiple Device Modules are composed of up to 8 device profile segments. In this way it is possible to build devices with multiple functionality. The different device profile entries are indexed at increments of 800h.
  - For Multiple Device Modules the object range 6000h to 9FFFh is sub-divided as follows:
    - 6000h to 67FFh device 0
    - 6800h to 6FFFh device 1
    - 7000h to 77FFh device 2
    - 7800h to 7FFFh device 3
    - 8000h to 87FFh device 4
    - 8800h to 8FFFh device 5
    - 9000h to 97FFh device 6
    - 9800h to 9FFFh device 7
- Space is left in the Object Dictionary at indices 2000h through 5FFFh for truly manufacturer-specific functionality.

A *16-bit index* is used to address all entries within the Object Dictionary. In the case of a simple variable the index references the value of this variable directly. In the case of records and arrays, however, the index addresses the whole data structure. To allow individual elements of structures of data to be accessed via the network *a sub-index is defined*. For single Object Dictionary entries such as an UNSIGNED8, BOOLEAN, INTEGER32 etc. the value for the sub-index is always zero. For complex Object Dictionary entries such as arrays or records with multiple data fields the sub-index references fields within a data-structure pointed to by the main index.

### Communication Model (2.3)

The communication model supports the transmission of isochronous and asynchronous frames. Isochronous frames are supported in POWERLINK Mode only, asynchronous frames in POWERLINK Mode and Basic Ethernet Mode.

The isochronous transmission of frames is supported by the POWERLINK Mode cycle structure. The system is synchronised by SoC frames. Asynchronous frames may be transmitted in the asynchronous slot of POWERLINK Mode cycle upon transmission grant by the POWERLINK MN, or at any time in Basic Ethernet Mode.

With respect to their functionality, three types of communication relationships are distinguished

- Master/Slave relationship
- Client/Server relationship
- Producer/Consumer relationship

POWERLINK collects more than one function into one frame (refer 4.6). It is therefore not usually possible to apply a single communication relationship to the complete frame, but only to particulars services inside the frame.

### Physical Layer (3)

Autonegotiation is not recommended.

To fit POWERLINK jitter requirements it is recommended to use hubs to build a POWERLINK network. Class 2 Repeaters shall be used in this case. Hubs may be integrated in the POWERLINK interface cards. Hub integration shall be indicated by **D_PHY_HubIntegrated_BOOL**. The number of externally accessible POWERLINK ports provided by a device shall be indicated by **D_PHY_ExtEPLPorts_U8**.

Switches may be used to build a POWERLINK network. The additional latency and jitter of switches has to be considered for system configuration. It has to be considered that **any POWERLINK network constructed with anything but Class 2 Repeater Devices does not conform to the POWERLINK standard as defined in this document**.

The MN uses a timeout after sending a PollRequest Frame to detect transmission errors and node failures. The round trip latency between the MN and a CN shall not exceed the timeout value. The timeout value can be set for every single node.

### Data Link Layer (4)

Two operating modes are defined for POWERLINK networks:

1. POWERLINK mode
  - In POWERLINK Mode network traffic follows the set of rules given in this standard for Real-time Ethernet communication. Network access is managed by a master, the POWERLINK Managing Node (MN). *A node can only be granted the right to send data on the network via the MN*. The central access rules preclude collisions, **the network is therefore deterministic in POWERLINK Mode**.
  - In POWERLINK Mode most communication transactions are via POWERLINK-specific messages. An asynchronous slot is available for non-POWERLINK frames. UDP/IP is the preferred data exchange mechanism in the asynchronous slot; however, it is possible to use any protocol.
2. Basic Ethernet mode
  - In Basic Ethernet Mode network communication follows the rules of Legacy Ethernet (IEEE802.3). Network access is via CSMA/CD. Collisions occur, and network traffic is nondeterministic. Any protocol on top of Ethernet may be used in Basic Ethernet mode, the preferred mechanisms for data exchange between nodes being UDP/IP and TCP/IP.

#### Powerlink Mode (4.2)

Controlled Nodes shall be only allowed to send when requested to by the MN. The Controlled Nodes shall be accessed cyclically by the MN. Unicast data shall be sent from the MN to each configured CN (frame: PReq), which shall then publish its data via multicast to all other nodes (frame: PRes). 

Optionally, the MN may send a multicast Pres frame in the isochrononous phase (see Fig. 19). With this frame the MN may publish its own data to all other nodes.
All available nodes in the network shall be configured on the MN. **Only one active MN is permitted in a POWERLINK network**.

The ability of a node to perform MN functions shall be indicated by the device description entry *D_DLL_FeatureMN_BOOL*.
The ability of a node to perform CN functions shall be indicated by the device description entry *D_DLL_FeatureCN_BOOL*.

CNs may be accessed every cycle *or every nth cycle* (multiplexed nodes, n > 1).

`PReq` can only be received by the specifically addressed CN. However, **`PRes` frames shall be sent by the CN as multicast messages**, allowing all other CNs to monitor the data being sent.

The ability of a CN to perform isochronous communication shall be indicated by a feature flag in the object dictionary entry *NMT_FeatureFlags_U32* (1F82h) and the device description entry *D_NMT_Isochronous_BOOL*.

The MN shall cyclically poll each async-only CN during the asynchronous phase with a StatusRequest – a special form of the SoA frame. The CN shall respond with a StatusResponse, special form of Asynchronous Send frame. The poll interval shall be at least C_NMT_STATREQ_CYCLE. 

Async-only CNs shall request the right to transmit asynchronous data from the MN, if required. Async-only CNs shall actively communicate during the asynchronous phase only. Nevertheless, they may listen to the multicast network traffic, transmitted by the MN and the isochronous CNs.

#### Services (4.2.3)

POWERLINK provides three services:
- Isochronous Data Transfer: One pair of messages per node shall be delivered every cycle, or every nth cycle in the case of multiplexed CNs. Additionally, there may be one multicast PRes message from the MN per cycle. Isochronous data transfer is typically used for the exchange of time critical data (real-time data).
- Asynchronous Data Transfer: **There may be one asynchronous message per cycle**. The right to send shall be assigned to a requesting node by the MN via the SoA message. Asynchronous data transfer is used for the exchange of non time-critical data.
- Synchronisation of all nodes: At the beginning of each isochronous phase, the MN transmits the multicast SoC message very precisely to synchronise all nodes in the network.

#### Powerlink Cycle

Isochronous cycle:
[Powerlink Cycle](powerlink_cycle.png)

**All data transfers shall be unconfirmed**, i.e. there is no confirmation that sent data has been received. To maintain deterministic behavior, protecting the isochronous data (PReq and PRes) is neither necessary nor desired. Asynchronous data may be protected by higher protocol layers.

`PReq` shall be an Ethernet *unicast* frame. It is received by the target node only. `PRes` shall be sent as an Ethernet *multicast* frame.

**Both the PReq and the PRes frames may transfer application data**.

Support of PRes transmission by the MN is optional. The ability of an MN to transmit PRes shall be indicated by the device description entry *D_DLL_MNFeaturePResTx_BOOL*. If the feature is provided, transmission shall be enabled by *NMT_NodeAssignment_AU32[C_ADR_MN_DEF_NODE_ID ].Bit 12*.

The isochronous phase shall be calculated from start of SoC to start of SoA.

The order in which CNs are polled may be implementation specific or controlled by object *NMT_IsochrSlotAssign_AU8* if supported by the MN. An implementation should pack the performed PReq / PRes packages to the begin of the isochronous phase. *It should provide means to rearrange the poll order*, **to avoid location of the nodes having the worst SoC latency time value** (*D_NMT_CNSoC2PReq_U32*) at the slot following SoC.

**Multiplexed timeslots:** POWERLINK supports CN communication classes, that determine the cycles in which nodes are to be addressed.

- Continuous: Continuous data shall be exchanged in every POWERLINK cycle.
- Multiplexed: Multiplexed data to and from one CN shall not be exchanged in every POWERLINK cycle. The accesses to the multiplexed CNs shall be dispersed to the multiplexed cycle that consists of a number of POWERLINK cycles. 

**The apportionment of the isochronous phase to continuous and multiplexed slots shall be fixed by configuration** (*NMT_MultiplCycleAssign_AU8*, *NMT_IsochrSlotAssign_AU8*).

In case of MN cycle loss, the multiplexed access sequence shall be continued on a per time base, after the cycle loss error phase is over. E.g. CNs shall be skipped to maintain time equidistance of access to nodes not affected by the cycle loss.

The ability of an MN enabled node to perform control of multiplexed isochronous operation shall be indicated by the device description entry *D_DLL_MNFeatureMultiplex_BOOL*. The ability of a CN enabled node to be isochronously accessed in a multiplexed way shall be indicated by the device description entry *D_DLL_CNFeatureMultiplex_BOOL*.

> Parei na pagina 43, secao 4.2.4.1.2.
