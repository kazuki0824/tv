use super::{AvDataId, AvSlotId};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AvMediaEventDraft { pub data_id: AvDataId, pub slot_id: AvSlotId, pub offset: usize, pub data_length: usize }
