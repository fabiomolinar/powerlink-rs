use crate::frame::basic::{EthernetHeader, MacAddress, ETHERNET_HEADER_SIZE};
use crate::common::{NetTime, RelativeTime};
use crate::types::{
    NodeId, C_ADR_MN_DEF_NODE_ID, C_DLL_MULTICAST_SOA,
    C_DLL_MULTICAST_SOC, MessageType, C_ADR_BROADCAST_NODE_ID,
    EPLVersion,
};
use crate::nmt::states::{NmtState};
use alloc::vec::Vec;
use super::codec::{Codec, CodecHelpers};
use crate::PowerlinkError;


// --- Start of Cycle (SoC) ---

/// Represents a complete SoC frame.
/// (Reference: EPSG DS 301, Section 4.6.1.1.2)
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
/// (Reference: EPSG DS 301, Table 16)
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

impl Codec for SocFrame {
    fn serialize(&self, buffer: &mut [u8]) -> Result<usize, PowerlinkError> {
        const SOC_SIZE: usize = 60;
        if buffer.len() < SOC_SIZE { return Err(PowerlinkError::BufferTooShort); }

        CodecHelpers::serialize_eth_header(&self.eth_header, buffer);
        CodecHelpers::serialize_pl_header(self.message_type, self.destination, self.source, buffer);

        buffer[17] = 0;
        let mut octet4 = 0u8;
        if self.flags.mc { octet4 |= 1 << 7; }
        if self.flags.ps { octet4 |= 1 << 6; }
        buffer[18] = octet4;
        buffer[19] = 0;
        buffer[20..28].copy_from_slice(&self.net_time.seconds.to_le_bytes());
        buffer[28..36].copy_from_slice(&self.net_time.nanoseconds.to_le_bytes());
        buffer[36..44].copy_from_slice(&self.relative_time.seconds.to_le_bytes());
        buffer[44..52].copy_from_slice(&self.relative_time.nanoseconds.to_le_bytes());
        
        Ok(SOC_SIZE)
    }

    fn deserialize(buffer: &[u8]) -> Result<Self, PowerlinkError> {
        if buffer.len() < 60 { return Err(PowerlinkError::BufferTooShort); }

        let eth_header = CodecHelpers::deserialize_eth_header(buffer)?;
        let (message_type, destination, source) = CodecHelpers::deserialize_pl_header(buffer)?;

        let octet4 = buffer[18];
        let flags = SocFlags {
            mc: (octet4 & (1 << 7)) != 0,
            ps: (octet4 & (1 << 6)) != 0,
        };

        let net_time = NetTime {
            seconds: u32::from_le_bytes(buffer[20..24].try_into()?),
            nanoseconds: u32::from_le_bytes(buffer[24..28].try_into()?),
        };

        let relative_time = RelativeTime {
            seconds: u32::from_le_bytes(buffer[28..32].try_into()?),
            nanoseconds: u32::from_le_bytes(buffer[32..36].try_into()?),
        };

        Ok(Self { eth_header, message_type, destination, source, flags, net_time, relative_time })
    }
}

// --- Start of Asynchronous (SoA) ---

/// Requested Service IDs for SoA frames.
/// (Reference: EPSG DS 301, Appendix 3.4)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RequestedServiceId {
    /// Corresponds to `NO_SERVICE`.
    NoService = 0x00,
    /// Corresponds to `IDENT_REQUEST`.
    IdentRequest = 0x01, 
    /// Corresponds to `STATUS_REQUEST`.
    StatusRequest = 0x02, 
    /// Corresponds to `NMT_REQUEST_INVITE`.
    NmtRequestInvite = 0x03,      
    /// Corresponds to `UNSPECIFIED_INVITE`.
    UnspecifiedInvite = 0xFF, 
}

impl TryFrom<u8> for RequestedServiceId {
    type Error = PowerlinkError;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x00 => Ok(Self::NoService),
            0x01 => Ok(Self::IdentRequest),
            0x02 => Ok(Self::StatusRequest),
            0x03 => Ok(Self::NmtRequestInvite),
            0xFF => Ok(Self::UnspecifiedInvite),
            _ => Err(PowerlinkError::InvalidFrame),
        }
    }
}

/// Represents a complete SoA frame.
/// (Reference: EPSG DS 301, Section 4.6.1.1.5)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SoAFrame {
    pub eth_header: EthernetHeader,
    pub message_type: MessageType,
    pub destination: NodeId,
    pub source: NodeId,
    pub nmt_state: NmtState,
    pub flags: SoAFlags,
    pub req_service_id: RequestedServiceId,
    pub target_node_id: NodeId,
    pub epl_version: EPLVersion,
}

/// Flags specific to the SoA frame.
/// (Reference: EPSG DS 301, Table 22)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SoAFlags {
    pub ea: bool, // Exception Acknowledge
    pub er: bool, // Exception Reset
}

impl SoAFrame {
    /// Creates a new SoA frame.
    pub fn new(
        source_mac: MacAddress,
        nmt_state: NmtState,
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

impl Codec for SoAFrame {
    fn serialize(&self, buffer: &mut [u8]) -> Result<usize, PowerlinkError> {
        const SOA_SIZE: usize = 60;        
        if buffer.len() < SOA_SIZE { return Err(PowerlinkError::BufferTooShort); }

        CodecHelpers::serialize_eth_header(&self.eth_header, buffer);
        CodecHelpers::serialize_pl_header(self.message_type, self.destination, self.source, buffer);

        buffer[17] = self.nmt_state as u8;        
        let mut octet4 = 0u8;
        if self.flags.ea { octet4 |= 1 << 2; }
        if self.flags.er { octet4 |= 1 << 1; }
        buffer[18] = octet4;
        buffer[19] = 0;
        buffer[20] = self.req_service_id as u8;
        buffer[21] = self.target_node_id.0;
        buffer[22] = self.epl_version.0;
        Ok(SOA_SIZE)
    }
    
    fn deserialize(buffer: &[u8]) -> Result<Self, PowerlinkError> {
        if buffer.len() < 60 { return Err(PowerlinkError::BufferTooShort); }

        let eth_header = CodecHelpers::deserialize_eth_header(buffer)?;
        let (message_type, destination, source) = CodecHelpers::deserialize_pl_header(buffer)?;
        let nmt_state = NmtState::try_from(buffer[17])?;

        let octet4 = buffer[18];
        let flags = SoAFlags {
            ea: (octet4 & (1 << 2)) != 0,
            er: (octet4 & (1 << 1)) != 0,
        };
        
        let req_service_id = RequestedServiceId::try_from(buffer[20])?;
        let target_node_id = NodeId(buffer[21]);
        let epl_version = EPLVersion(buffer[22]);

        Ok(Self { eth_header, message_type, destination, source, nmt_state, flags, req_service_id, target_node_id, epl_version })
    }
}

// --- Asynchronous Send (ASnd) ---

/// Service IDs for ASnd frames.
/// (Reference: EPSG DS 301, Appendix 3.3)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ServiceId {
    /// Corresponds to `IDENT_RESPONSE`.
    IdentResponse = 0x01,
    /// Corresponds to `STATUS_RESPONSE`.
    StatusResponse = 0x02, 
    /// Corresponds to `NMT_REQUEST`.
    NmtRequest = 0x03, 
    /// Corresponds to `NMT_COMMAND`.
    NmtCommand = 0x04,      
    /// Corresponds to `SDO`.
    Sdo = 0x05, 
}

impl TryFrom<u8> for ServiceId {
    type Error = PowerlinkError;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x01 => Ok(Self::IdentResponse),
            0x02 => Ok(Self::StatusResponse),
            0x03 => Ok(Self::NmtRequest),
            0x04 => Ok(Self::NmtCommand),
            0x05 => Ok(Self::Sdo),
            _ => Err(PowerlinkError::InvalidFrame),
        }
    }
}

/// Represents a complete ASnd frame.
/// (Reference: EPSG DS 301, Section 4.6.1.1.6)
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

impl Codec for ASndFrame {
    fn serialize(&self, buffer: &mut [u8]) -> Result<usize, PowerlinkError> {
        let header_size = ETHERNET_HEADER_SIZE + 4; // 4 bytes for PL header
        let total_size = header_size + self.payload.len();
        if buffer.len() < total_size { return Err(PowerlinkError::FrameTooLarge); }
        
        // Ethernet Header
        buffer[0..6].copy_from_slice(&self.eth_header.destination_mac.0);
        buffer[6..12].copy_from_slice(&self.eth_header.source_mac.0);
        buffer[12..14].copy_from_slice(&self.eth_header.ether_type.to_be_bytes());
        
        // POWERLINK Data
        buffer[14] = self.message_type as u8;
        buffer[15] = self.destination.0;
        buffer[16] = self.source.0;
        buffer[17] = self.service_id as u8;
        
        // Payload
        buffer[header_size..total_size].copy_from_slice(&self.payload);
        
        Ok(total_size)
    }

    fn deserialize(buffer: &[u8]) -> Result<Self, PowerlinkError> {
        let header_size = ETHERNET_HEADER_SIZE + 4;
        if buffer.len() < header_size { return Err(PowerlinkError::BufferTooShort); }

        let eth_header = CodecHelpers::deserialize_eth_header(buffer)?;
        let (message_type, destination, source) = CodecHelpers::deserialize_pl_header(buffer)?;
        let service_id = ServiceId::try_from(buffer[17])?;
        
        let payload = buffer[header_size..].to_vec();

        Ok(Self { eth_header, message_type, destination, source, service_id, payload })
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
        let service = RequestedServiceId::StatusRequest;
        let flags = SoAFlags { ea: true, er: false };
        
        let frame = SoAFrame::new(
            source_mac, NmtState::NmtCsNotActive, flags,
            service, target_node, EPLVersion(1)
        );

        assert_eq!(frame.eth_header.destination_mac.0, C_DLL_MULTICAST_SOA);
        assert_eq!(frame.eth_header.source_mac, source_mac);
        assert_eq!(frame.message_type, MessageType::SoA);
        assert_eq!(frame.source, NodeId(C_ADR_MN_DEF_NODE_ID));
    }
}