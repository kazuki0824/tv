pub mod callback_bridge;
pub mod callback_slot;
pub mod callback_store;
pub mod demux_object;
pub mod descrambler_object;
pub mod dvr_object;
pub mod frontend_callback_delivery;
pub mod child_object_open;
pub mod error_bridge;
pub mod filter_object;
pub mod frontend_object;
pub mod lnb_object;
pub mod native_handle_bridge;
pub mod object_handle;
pub mod object_runtime;
pub mod service_entry;
pub mod tuner_service;

pub use callback_bridge::{CallbackApi, CallbackBridge, CallbackFailureRecord, CallbackOwnerKind};
pub use callback_slot::{AidlCallbackSlot, AidlCallbackSlotError};
pub use callback_store::{
    clear_owner_callbacks, dvr_callback_for_owner, filter_callback_for_owner,
    frontend_callback_for_owner, retain_dvr_callback, retain_filter_callback,
    retain_frontend_callback, retain_lnb_callback, AidlCallbackStoreError,
};
pub use demux_object::DemuxAidlObject;
pub use descrambler_object::DescramblerAidlObject;
pub use dvr_object::DvrAidlObject;
pub use error_bridge::{AidlErrorBridge, AidlErrorMapping};
pub use filter_object::FilterAidlObject;
pub use frontend_object::FrontendAidlObject;
pub use lnb_object::LnbAidlObject;
pub use native_handle_bridge::{
    NativeHandleBridge, NativeHandleBridgeError, NativeHandleBridgeErrorKind,
    NativeHandleBridgeKind,
};
pub use object_handle::{
    AidlObjectGeneration, AidlObjectHandle, AidlObjectHandleError, AidlObjectId, AidlObjectKind,
};
pub use object_runtime::SharedTunerRuntime;
pub use service_entry::run_service;
pub use tuner_service::TunerAidlService;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_wrappers_keep_only_handles() {
        let frontend = FrontendAidlObject::new(
            AidlObjectHandle::new(
                AidlObjectKind::Frontend,
                AidlObjectId(1),
                AidlObjectGeneration(2),
            ),
            std::sync::Arc::new(std::sync::Mutex::new(
                maleicacid_tuner_hal2_service_runtime::TunerServiceRuntime::new(),
            )),
        )
        .unwrap();
        assert_eq!(frontend.handle().object_id(), AidlObjectId(1));
    }
}
