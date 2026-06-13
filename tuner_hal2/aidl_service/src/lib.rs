pub mod tuner_service;
pub mod service_entry;
pub mod frontend_object;
pub mod demux_object;
pub mod filter_object;
pub mod dvr_object;
pub mod descrambler_object;
pub mod lnb_object;
pub mod callback_bridge;
pub mod callback_slot;
pub mod callback_store;
pub mod native_handle_bridge;
pub mod error_bridge;
pub mod object_handle;
pub mod object_runtime;
pub mod input_snapshot;
pub mod aidl_v2_conversion_contract;

pub use tuner_service::TunerAidlService;
pub use service_entry::run_service;
pub use frontend_object::FrontendAidlObject;
pub use demux_object::DemuxAidlObject;
pub use filter_object::FilterAidlObject;
pub use dvr_object::DvrAidlObject;
pub use descrambler_object::DescramblerAidlObject;
pub use lnb_object::LnbAidlObject;
pub use callback_bridge::{CallbackApi, CallbackBridge, CallbackFailureRecord, CallbackOwnerKind};
pub use callback_slot::{AidlCallbackSlot, AidlCallbackSlotError};
pub use callback_store::{clear_owner_callbacks, retain_frontend_callback, retain_lnb_callback, AidlCallbackStoreError};
pub use native_handle_bridge::{NativeHandleBridge, NativeHandleBridgeError, NativeHandleBridgeErrorKind, NativeHandleBridgeKind};
pub use error_bridge::{AidlErrorBridge, AidlErrorMapping};
pub use object_handle::{AidlObjectGeneration, AidlObjectHandle, AidlObjectHandleError, AidlObjectId, AidlObjectKind};
pub use object_runtime::SharedTunerRuntime;
pub use aidl_v2_conversion_contract::{AIDL_V2_SCHEMA_HASH, AIDL_V2_SCHEMA_SOURCE};
pub use input_snapshot::{
    snapshot_av_stream_type, snapshot_demux_open_dvr, snapshot_demux_open_filter,
    snapshot_dvr_settings, snapshot_filter_delay_hint, snapshot_filter_settings,
    snapshot_strong_handle,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_wrappers_keep_only_handles() {
        let frontend = FrontendAidlObject::new(AidlObjectHandle::new(
            AidlObjectKind::Frontend,
            AidlObjectId(1),
            AidlObjectGeneration(2),
        ), std::sync::Arc::new(std::sync::Mutex::new(maleicacid_tuner_hal2_service_runtime::TunerServiceRuntime::new()))).unwrap();
        assert_eq!(frontend.handle().object_id(), AidlObjectId(1));
    }
}
