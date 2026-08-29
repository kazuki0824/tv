use super::{AvDataId, AvSlotId};
use std::fs::File;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AvMediaEventMetadata {
    pub stream_id: u8,
    pub is_pts_present: bool,
    pub pts_90khz: Option<u64>,
    pub is_dts_present: bool,
    pub dts_90khz: Option<u64>,
}

impl AvMediaEventMetadata {
    pub const fn from_pes(
        stream_id: u8,
        pts_90khz: Option<u64>,
        dts_90khz: Option<u64>,
    ) -> Self {
        Self {
            stream_id,
            is_pts_present: pts_90khz.is_some(),
            pts_90khz,
            is_dts_present: dts_90khz.is_some(),
            dts_90khz,
        }
    }

    pub const fn from_pes_with_authoritative_pts(
        stream_id: u8,
        pes_pts_90khz: Option<u64>,
        authoritative_pts_90khz: u64,
        dts_90khz: Option<u64>,
    ) -> Self {
        Self {
            stream_id,
            is_pts_present: pes_pts_90khz.is_some(),
            pts_90khz: Some(authoritative_pts_90khz),
            is_dts_present: dts_90khz.is_some(),
            dts_90khz,
        }
    }
}

#[derive(Clone, Debug)]
pub struct AvMediaEventDescriptor {
    pub data_id: AvDataId,
    pub slot_id: AvSlotId,
    pub offset: usize,
    pub data_length: usize,
    pub metadata: AvMediaEventMetadata,
    pub event_local_file: Option<Arc<File>>,
}

impl PartialEq for AvMediaEventDescriptor {
    fn eq(&self, other: &Self) -> bool {
        self.data_id == other.data_id
            && self.slot_id == other.slot_id
            && self.offset == other.offset
            && self.data_length == other.data_length
            && self.metadata == other.metadata
            && self.event_local_file.is_some() == other.event_local_file.is_some()
    }
}

impl Eq for AvMediaEventDescriptor {}
