pub mod event;
mod release_txn;
pub mod shared_backing;
pub mod slot;

pub use event::AvMediaEventDescriptor;
pub use release_txn::AvHandleReleaseOutcome;
pub(crate) use release_txn::{
    AvDataIdState, AvFilterReleaseState, AvHandleReleaseInput, AvHandleReleaseTxn,
};
pub use shared_backing::{
    AvPayloadDeliveryOutcome, AvSharedBacking, AvSharedBackingError, AvSharedHandleExport,
    ClientHandleState, DEFAULT_AV_SHARED_SLOT_COUNT, DEFAULT_AV_SHARED_SLOT_SIZE_BYTES,
};
pub use slot::{AvDataId, AvSlotId};
