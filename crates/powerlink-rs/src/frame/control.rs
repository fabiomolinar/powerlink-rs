#![allow(non_camel_case_types)]

use crate::frame::basic::{EthernetHeader, MacAddress};
use crate::common::{NetTime, RelativeTime};
use crate::types::{
    NodeId, C_ADR_MN_DEF_NODE_ID, C_DLL_MULTICAST_SOA, 
    C_DLL_MULTICAST_SOC, MessageType, C_ADR_BROADCAST_NODE_ID, 
    EPLVersion
};
use crate::nmt::states::{NMTState};
use alloc::vec::Vec;


// --- Start of Cycle (SoC) ---

/// Represents a complete SoC frame.
/// (EPSG DS 301, Section 4.6.1.1.2)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SocFrame {
    pub eth_header: EthernetHeader,
    pub message_type: MessageType,
    pub destination: NodeId,
    pub source: NodeId,
    pub flags: SocFlags,
    pub net_time: NetTime,
    pub relative_time: RelativeTime,
}

/// Flags specific to the SoC frame.
/// (EPSG DS 301, Table 16)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SocFlags {
    pub mc: bool, // Multiplexed Cycle Completed
    pub ps: bool, // Prescaled Slot
}

impl SocFrame {
    /// Creates a new SoC frame.
    pub fn new(
        source_mac: MacAddress,
        flags: SocFlags,
        net_time: NetTime,
        relative_time: RelativeTime,
    ) -> Self {
        let eth_header = EthernetHeader::new(
            MacAddress(C_DLL_MULTICAST_SOC), 
            source_mac
        );                
        
        SocFrame {
            eth_header,
            message_type: MessageType::SoC,
            destination: NodeId(C_ADR_BROADCAST_NODE_ID),
            source: NodeId(C_ADR_MN_DEF_NODE_ID),
            flags,
            net_time,
            relative_time,
        }
    }
}

// --- Start of Asynchronous (SoA) ---

/// Requested Service IDs for SoA frames.
/// (EPSG DS 301, Appendix 3.4)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RequestedServiceId {
    NO_SERVICE = 0x00,
    IDENT_REQUEST = 0x01, 
    STATUS_REQUEST = 0x02, 
    NMT_REQUEST_INVITE = 0x03,         
    UNSPECIFIED_INVITE = 0xFF, 
}

/// Represents a complete SoA frame.
/// (EPSG DS 301, Section 4.6.1.1.5)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SoAFrame {
    pub eth_header: EthernetHeader,
    pub message_type: MessageType,
    pub destination: NodeId,
    pub source: NodeId,
    pub nmt_state: NMTState,
    pub flags: SoAFlags,
    pub req_service_id: RequestedServiceId,
    pub target_node_id: NodeId,
    pub epl_version: EPLVersion,
}

/// Flags specific to the SoA frame.
/// (EPSG DS 301, Table 22)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SoAFlags {
    pub ea: bool, // Exception Acknowledge
    pub er: bool, // Exception Reset
}

impl SoAFrame {
    /// Creates a new SoA frame.
    pub fn new(
        source_mac: MacAddress,
        nmt_state: NMTState,
        flags: SoAFlags,
        requested_service: RequestedServiceId,
        target_node_id: NodeId,
        epl_version: EPLVersion,
    ) -> Self {        
        let eth_header = EthernetHeader::new(
            MacAddress(C_DLL_MULTICAST_SOA), 
            source_mac
        );

        SoAFrame { 
            eth_header,
            message_type: MessageType::SoA,
            destination: NodeId(C_ADR_BROADCAST_NODE_ID),
            source: NodeId(C_ADR_MN_DEF_NODE_ID),
            nmt_state,
            flags,
            req_service_id: requested_service,
            target_node_id,
            epl_version,
         }
    }
}

// --- Asynchronous Send (ASnd) ---

/// Service IDs for ASnd frames.
/// (EPSG DS 301, Appendix 3.3)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ServiceId {
    IDENT_RESPONSE = 0x01,
    STATUS_RESPONSE = 0x02, 
    NMT_REQUEST = 0x03, 
    NMT_COMMAND = 0x04,         
    SDO = 0x05, 
}

/// Represents a complete ASnd frame.
/// (EPSG DS 301, Section 4.6.1.1.6)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ASndFrame {
    pub eth_header: EthernetHeader,
    pub message_type: MessageType,
    pub destination: NodeId,
    pub source: NodeId,
    pub service_id: ServiceId,
    pub payload: Vec<u8>,
}

impl ASndFrame {
    /// Creates a new ASnd frame.
    pub fn new(
        source_mac: MacAddress,
        dest_mac: MacAddress,
        target_node_id: NodeId,
        source_node_id: NodeId,
        service_id: ServiceId,
        payload: Vec<u8>,
    ) -> Self {
        let eth_header = EthernetHeader::new(dest_mac, source_mac);                
        
        ASndFrame { 
            eth_header,
            message_type: MessageType::ASnd,
            destination: target_node_id,
            source: source_node_id,
            service_id,
            payload,
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{C_DLL_MULTICAST_SOC, C_DLL_MULTICAST_SOA};
    
    #[test]
    fn test_socframe_new_constructor() {
        let source_mac = MacAddress([0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC]);
        let dummy_time = NetTime{seconds: 0xABCD, nanoseconds: 0xABCD};
        let dummy_rel_time = RelativeTime{seconds: 0xABCD, nanoseconds: 0xABCD};
        let flags = SocFlags { mc: true, ps: false };
        let frame = SocFrame::new(source_mac, flags, dummy_time, dummy_rel_time);

        assert_eq!(frame.eth_header.destination_mac.0, C_DLL_MULTICAST_SOC);
        assert_eq!(frame.eth_header.source_mac, source_mac);
        assert_eq!(frame.message_type, MessageType::SoC);
        assert_eq!(frame.source, NodeId(C_ADR_MN_DEF_NODE_ID));
        assert_eq!(frame.destination, NodeId(C_ADR_BROADCAST_NODE_ID));
        assert_eq!(frame.flags.mc, true);
        assert_eq!(frame.flags.ps, false);
    }
    
    #[test]
    fn test_soaframe_new_constructor() {
        let source_mac = MacAddress([0xFE, 0xDC, 0xBA, 0x98, 0x76, 0x54]);
        let target_node = NodeId(42);
        let service = RequestedServiceId::STATUS_REQUEST;
        let flags = SoAFlags { ea: true, er: false };
        
        let frame = SoAFrame::new(
            source_mac, NMTState::NotActive, flags,
            service, target_node, EPLVersion(1)
        );

        assert_eq!(frame.eth_header.destination_mac.0, C_DLL_MULTICAST_SOA);
        assert_eq!(frame.eth_header.source_mac, source_mac);
        assert_eq!(frame.message_type, MessageType::SoA);
        assert_eq!(frame.source, NodeId(C_ADR_MN_DEF_NODE_ID));
    }
}