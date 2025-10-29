use crate::sdo::command::SdoCommand;
use alloc::vec::Vec;

/// The state of an SDO connection from the server's perspective.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SdoServerState {
    #[default]
    Closed,
    Opening,
    Established,
    SegmentedDownload(SdoTransferState),
    SegmentedUpload(SdoTransferState),
}

/// Holds the context for an ongoing segmented transfer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdoTransferState {
    pub(super) transaction_id: u8,
    pub(super) total_size: usize,
    pub(super) data_buffer: Vec<u8>,
    // For uploads, this is the offset of the next byte to be sent.
    // For downloads, this tracks bytes received.
    pub(super) offset: usize,
    pub(super) index: u16,
    pub(super) sub_index: u8,
    // New fields for retransmission logic
    pub(super) deadline_us: Option<u64>,
    pub(super) retransmissions_left: u32,
    pub(super) last_sent_segment: Option<SdoCommand>,
}
