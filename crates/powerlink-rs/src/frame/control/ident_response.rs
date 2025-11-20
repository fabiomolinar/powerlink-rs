// crates/powerlink-rs/src/frame/control/ident_response.rs
//! Defines the structure and codec for the IdentResponse service payload.

use crate::frame::poll::{PRFlag, RSFlag};
use crate::hal::PowerlinkError;
use crate::nmt::flags::FeatureFlags;
use crate::nmt::states::NmtState;
use crate::od::ObjectDictionary;
use crate::od::constants; // Import constants
use crate::types::{EPLVersion, IpAddress, UNSIGNED16, UNSIGNED32};
use alloc::string::{String, ToString};
use core::convert::TryInto;
use log::warn;

const IDENT_RESPONSE_PAYLOAD_SIZE: usize = 158;
const HOSTNAME_OFFSET: usize = 78;
const HOSTNAME_SIZE: usize = 32;

/// Represents the payload of an ASnd(IdentResponse) frame.
///
/// This structure contains all fields defined in the NMT Service Slot
/// for an IdentResponse.
/// (Reference: EPSG DS 301, Section 7.3.3.2.1, Table 135)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentResponsePayload {
    // Octet 1: Flags
    pub pr: PRFlag,
    pub rs: RSFlag,
    // Octet 2: NMTState
    pub nmt_state: NmtState,
    // Octet 4: EPLVersion
    pub epl_version: EPLVersion,
    // Octets 6-9: FeatureFlags
    pub feature_flags: FeatureFlags,
    // Octets 10-11: MTU
    pub mtu: UNSIGNED16,
    // Octets 12-13: PollInSize (PReq payload)
    pub poll_in_size: UNSIGNED16,
    // Octets 14-15: PollOutSize (PRes payload)
    pub poll_out_size: UNSIGNED16,
    // Octets 16-19: ResponseTime (PRes latency)
    pub response_time: UNSIGNED32,
    // Octets 22-25: DeviceType
    pub device_type: UNSIGNED32,
    // Octets 26-29: VendorID
    pub vendor_id: UNSIGNED32,
    // Octets 30-33: ProductCode
    pub product_code: UNSIGNED32,
    // Octets 34-37: RevisionNumber
    pub revision_number: UNSIGNED32,
    // Octets 38-41: SerialNumber
    pub serial_number: UNSIGNED32,
    // Octets 50-53: VerifyConfigurationDate
    pub verify_conf_date: UNSIGNED32,
    // Octets 54-57: VerifyConfigurationTime
    pub verify_conf_time: UNSIGNED32,
    // Octets 58-61: ApplicationSwDate
    pub app_sw_date: UNSIGNED32,
    // Octets 62-65: ApplicationSwTime
    pub app_sw_time: UNSIGNED32,
    // Octets 66-69: IPAddress
    pub ip_address: IpAddress,
    // Octets 70-73: SubnetMask
    pub subnet_mask: IpAddress,
    // Octets 74-77: DefaultGateway
    pub default_gateway: IpAddress,
    // Octets 78-109: HostName (32 bytes)
    pub host_name: String,
}

impl IdentResponsePayload {
    /// Creates a new `IdentResponsePayload` by reading all required
    /// values from the Object Dictionary.
    pub fn new(od: &ObjectDictionary) -> Self {
        // Read all values from the OD, providing 0 or an empty string as a fallback.
        let nmt_state = od
            .read_u8(constants::IDX_NMT_CURR_NMT_STATE_U8, 0)
            .and_then(|val| NmtState::try_from(val).ok())
            .unwrap_or(NmtState::NmtNotActive);

        let epl_version = EPLVersion(
            od.read_u8(constants::IDX_NMT_EPL_VERSION_U8, 0)
                .unwrap_or(0),
        );
        let feature_flags = FeatureFlags::from_bits_truncate(
            od.read_u32(constants::IDX_NMT_FEATURE_FLAGS_U32, 0)
                .unwrap_or(0),
        );
        let mtu = od
            .read_u16(
                constants::IDX_NMT_CYCLE_TIMING_REC,
                constants::SUBIDX_NMT_CYCLE_TIMING_ASYNC_MTU_U16,
            )
            .unwrap_or(0);
        let poll_in_size = od
            .read_u16(
                constants::IDX_NMT_CYCLE_TIMING_REC,
                constants::SUBIDX_NMT_CYCLE_TIMING_PREQ_ACT_PAYLOAD_U16,
            )
            .unwrap_or(0);
        let poll_out_size = od
            .read_u16(
                constants::IDX_NMT_CYCLE_TIMING_REC,
                constants::SUBIDX_NMT_CYCLE_TIMING_PRES_ACT_PAYLOAD_U16,
            )
            .unwrap_or(0);
        let response_time = od
            .read_u32(
                constants::IDX_NMT_CYCLE_TIMING_REC,
                constants::SUBIDX_NMT_CYCLE_TIMING_PRES_MAX_LATENCY_U32,
            )
            .unwrap_or(0);
        let device_type = od
            .read_u32(constants::IDX_NMT_DEVICE_TYPE_U32, 0)
            .unwrap_or(0);
        let vendor_id = od
            .read_u32(constants::IDX_NMT_IDENTITY_OBJECT_REC, 1)
            .unwrap_or(0);
        let product_code = od
            .read_u32(constants::IDX_NMT_IDENTITY_OBJECT_REC, 2)
            .unwrap_or(0);
        let revision_number = od
            .read_u32(constants::IDX_NMT_IDENTITY_OBJECT_REC, 3)
            .unwrap_or(0);
        let serial_number = od
            .read_u32(constants::IDX_NMT_IDENTITY_OBJECT_REC, 4)
            .unwrap_or(0);
        let verify_conf_date = od
            .read_u32(constants::IDX_CFM_VERIFY_CONFIG_REC, 1)
            .unwrap_or(0);
        let verify_conf_time = od
            .read_u32(constants::IDX_CFM_VERIFY_CONFIG_REC, 2)
            .unwrap_or(0);
        let app_sw_date = od
            .read_u32(constants::IDX_PDL_LOC_VER_APPL_SW_REC, 1)
            .unwrap_or(0);
        let app_sw_time = od
            .read_u32(constants::IDX_PDL_LOC_VER_APPL_SW_REC, 2)
            .unwrap_or(0);
        let ip_address = od
            .read_u32(constants::IDX_NWL_IP_ADDR_TABLE_REC, 2)
            .unwrap_or(0)
            .to_le_bytes();
        let subnet_mask = od
            .read_u32(constants::IDX_NWL_IP_ADDR_TABLE_REC, 3)
            .unwrap_or(0)
            .to_le_bytes();
        let default_gateway = od
            .read_u32(constants::IDX_NWL_IP_ADDR_TABLE_REC, 5)
            .unwrap_or(0)
            .to_le_bytes();
        let host_name = od
            .read(constants::IDX_NMT_HOST_NAME_VSTR, 0)
            .and_then(|cow| match &*cow {
                crate::od::ObjectValue::VisibleString(s) => Some(s.clone()),
                _ => None,
            })
            .unwrap_or_default();

        Self {
            pr: PRFlag::default(), // PR/RS flags are set by the caller (e.g., in cn/payload.rs)
            rs: RSFlag::default(),
            nmt_state,
            epl_version,
            feature_flags,
            mtu,
            poll_in_size,
            poll_out_size,
            response_time,
            device_type,
            vendor_id,
            product_code,
            revision_number,
            serial_number,
            verify_conf_date,
            verify_conf_time,
            app_sw_date,
            app_sw_time,
            ip_address,
            subnet_mask,
            default_gateway,
            host_name,
        }
    }

    /// Serializes the `IdentResponsePayload` into the provided buffer.
    ///
    /// Returns the number of bytes written, which is always `IDENT_RESPONSE_PAYLOAD_SIZE`.
    pub fn serialize(&self, buffer: &mut [u8]) -> Result<usize, PowerlinkError> {
        if buffer.len() < IDENT_RESPONSE_PAYLOAD_SIZE {
            return Err(PowerlinkError::BufferTooShort);
        }

        // Fill buffer with zeros (for all reserved fields)
        buffer[..IDENT_RESPONSE_PAYLOAD_SIZE].fill(0);

        // Octet 1: PR/RS Flags
        buffer[1] = (self.pr as u8) << 3 | self.rs.get();
        // Octet 2: NMTState
        buffer[2] = self.nmt_state as u8;
        // Octet 4: EPLVersion
        buffer[4] = self.epl_version.0;
        // Octets 6-9: FeatureFlags
        buffer[6..10].copy_from_slice(&self.feature_flags.0.to_le_bytes());
        // Octets 10-11: MTU
        buffer[10..12].copy_from_slice(&self.mtu.to_le_bytes());
        // Octets 12-13: PollInSize
        buffer[12..14].copy_from_slice(&self.poll_in_size.to_le_bytes());
        // Octets 14-15: PollOutSize
        buffer[14..16].copy_from_slice(&self.poll_out_size.to_le_bytes());
        // Octets 16-19: ResponseTime
        buffer[16..20].copy_from_slice(&self.response_time.to_le_bytes());
        // Octets 22-25: DeviceType
        buffer[22..26].copy_from_slice(&self.device_type.to_le_bytes());
        // Octets 26-29: VendorID
        buffer[26..30].copy_from_slice(&self.vendor_id.to_le_bytes());
        // Octets 30-33: ProductCode
        buffer[30..34].copy_from_slice(&self.product_code.to_le_bytes());
        // Octets 34-37: RevisionNumber
        buffer[34..38].copy_from_slice(&self.revision_number.to_le_bytes());
        // Octets 38-41: SerialNumber
        buffer[38..42].copy_from_slice(&self.serial_number.to_le_bytes());
        // Octets 50-53: VerifyConfigurationDate
        buffer[50..54].copy_from_slice(&self.verify_conf_date.to_le_bytes());
        // Octets 54-57: VerifyConfigurationTime
        buffer[54..58].copy_from_slice(&self.verify_conf_time.to_le_bytes());
        // Octets 58-61: ApplicationSwDate
        buffer[58..62].copy_from_slice(&self.app_sw_date.to_le_bytes());
        // Octets 62-65: ApplicationSwTime
        buffer[62..66].copy_from_slice(&self.app_sw_time.to_le_bytes());
        // Octets 66-69: IPAddress
        buffer[66..70].copy_from_slice(&self.ip_address);
        // Octets 70-73: SubnetMask
        buffer[70..74].copy_from_slice(&self.subnet_mask);
        // Octets 74-77: DefaultGateway
        buffer[74..78].copy_from_slice(&self.default_gateway);
        // Octets 78-109: HostName
        let hostname_bytes = self.host_name.as_bytes();
        let len = hostname_bytes.len().min(HOSTNAME_SIZE);
        buffer[HOSTNAME_OFFSET..HOSTNAME_OFFSET + len].copy_from_slice(&hostname_bytes[..len]);

        Ok(IDENT_RESPONSE_PAYLOAD_SIZE)
    }

    /// Deserializes an `IdentResponsePayload` from a byte slice.
    pub fn deserialize(buffer: &[u8]) -> Result<Self, PowerlinkError> {
        if buffer.len() < IDENT_RESPONSE_PAYLOAD_SIZE {
            warn!(
                "IdentResponse payload too short. Expected {}, got {}",
                IDENT_RESPONSE_PAYLOAD_SIZE,
                buffer.len()
            );
            return Err(PowerlinkError::BufferTooShort);
        }

        let octet1 = buffer[1];
        let pr = PRFlag::try_from(octet1 >> 3)?;
        let rs = RSFlag::new(octet1 & 0b111);

        let nmt_state = NmtState::try_from(buffer[2])?;
        let epl_version = EPLVersion(buffer[4]);

        let feature_flags =
            FeatureFlags::from_bits_truncate(u32::from_le_bytes(buffer[6..10].try_into()?));
        let mtu = u16::from_le_bytes(buffer[10..12].try_into()?);
        let poll_in_size = u16::from_le_bytes(buffer[12..14].try_into()?);
        let poll_out_size = u16::from_le_bytes(buffer[14..16].try_into()?);
        let response_time = u32::from_le_bytes(buffer[16..20].try_into()?);
        let device_type = u32::from_le_bytes(buffer[22..26].try_into()?);
        let vendor_id = u32::from_le_bytes(buffer[26..30].try_into()?);
        let product_code = u32::from_le_bytes(buffer[30..34].try_into()?);
        let revision_number = u32::from_le_bytes(buffer[34..38].try_into()?);
        let serial_number = u32::from_le_bytes(buffer[38..42].try_into()?);
        let verify_conf_date = u32::from_le_bytes(buffer[50..54].try_into()?);
        let verify_conf_time = u32::from_le_bytes(buffer[54..58].try_into()?);
        let app_sw_date = u32::from_le_bytes(buffer[58..62].try_into()?);
        let app_sw_time = u32::from_le_bytes(buffer[62..66].try_into()?);
        let ip_address = buffer[66..70].try_into()?;
        let subnet_mask = buffer[70..74].try_into()?;
        let default_gateway = buffer[74..78].try_into()?;        

        // Parse HostName
        let hostname_slice = &buffer[HOSTNAME_OFFSET..HOSTNAME_OFFSET + HOSTNAME_SIZE];
        let len = hostname_slice
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(HOSTNAME_SIZE);
        let host_name = String::from_utf8_lossy(&hostname_slice[..len]).to_string();

        Ok(Self {
            pr,
            rs,
            nmt_state,
            epl_version,
            feature_flags,
            mtu,
            poll_in_size,
            poll_out_size,
            response_time,
            device_type,
            vendor_id,
            product_code,
            revision_number,
            serial_number,
            verify_conf_date,
            verify_conf_time,
            app_sw_date,
            app_sw_time,
            ip_address,
            subnet_mask,
            default_gateway,
            host_name,
        })
    }
}
