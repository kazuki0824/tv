use super::{AvDataId, AvSlotId};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AvMediaEventDescriptor { pub data_id: AvDataId, pub slot_id: AvSlotId, pub offset: usize, pub data_length: usize }
