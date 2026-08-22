use super::{AvDataId, AvSlotId};
use std::fs::File;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct AvMediaEventDescriptor {
    pub data_id: AvDataId,
    pub slot_id: AvSlotId,
    pub offset: usize,
    pub data_length: usize,
    pub event_local_file: Option<Arc<File>>,
}

impl PartialEq for AvMediaEventDescriptor {
    fn eq(&self, other: &Self) -> bool {
        self.data_id == other.data_id
            && self.slot_id == other.slot_id
            && self.offset == other.offset
            && self.data_length == other.data_length
            && self.event_local_file.is_some() == other.event_local_file.is_some()
    }
}

impl Eq for AvMediaEventDescriptor {}
