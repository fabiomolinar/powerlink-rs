// crates/powerlink-rs/src/sdo/command/payload.rs
use crate::PowerlinkError;

/// Specific data for a "Read by Index" request.
/// (Reference: EPSG DS 301, Table 61)
pub struct ReadByIndexRequest {
    pub index: u16,
    pub sub_index: u8,
}

impl ReadByIndexRequest {
    pub fn from_payload(payload: &[u8]) -> Result<Self, PowerlinkError> {
        if payload.len() < 4 {
            return Err(PowerlinkError::BufferTooShort);
        }
        Ok(Self {
            index: u16::from_le_bytes(payload[0..2].try_into()?),
            sub_index: payload[2],
        })
    }
}

/// Specific data for a "Write by Index" request.
/// (Reference: EPSG DS 301, Table 59)
pub struct WriteByIndexRequest<'a> {
    pub index: u16,
    pub sub_index: u8,
    pub data: &'a [u8],
}

impl<'a> WriteByIndexRequest<'a> {
    pub fn from_payload(payload: &'a [u8]) -> Result<Self, PowerlinkError> {
        if payload.len() < 4 {
            return Err(PowerlinkError::BufferTooShort);
        }
        Ok(Self {
            index: u16::from_le_bytes(payload[0..2].try_into()?),
            sub_index: payload[2],
            data: &payload[4..],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn test_read_by_index_request_parser() {
        let payload = vec![0x06, 0x10, 0x01, 0x00]; // Read 0x1006 sub 1
        let req = ReadByIndexRequest::from_payload(&payload).unwrap();
        assert_eq!(req.index, 0x1006);
        assert_eq!(req.sub_index, 1);

        let short_payload = vec![0x06, 0x10, 0x01];
        assert!(ReadByIndexRequest::from_payload(&short_payload).is_err());
    }

    #[test]
    fn test_write_by_index_request_parser() {
        let payload = vec![0x00, 0x60, 0x00, 0x00, 0xAA, 0xBB]; // Write 0x6000 sub 0 with data
        let req = WriteByIndexRequest::from_payload(&payload).unwrap();
        assert_eq!(req.index, 0x6000);
        assert_eq!(req.sub_index, 0);
        assert_eq!(req.data, &[0xAA, 0xBB]);
    }
}