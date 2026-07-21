pub(crate) mod callback_store;
pub(crate) mod child_object_open;
pub(crate) mod demux_object;
pub(crate) mod descrambler_object;
pub(crate) mod dvr_callback_delivery;
pub(crate) mod dvr_object;
pub(crate) mod error_bridge;
pub(crate) mod filter_callback_delivery;
pub(crate) mod filter_object;
pub(crate) mod frontend_callback_delivery;
pub(crate) mod frontend_object;
pub(crate) mod lnb_object;
pub(crate) mod object_handle;
pub(crate) mod object_runtime;
pub(crate) mod service_context;
pub(crate) mod service_entry;
pub(crate) mod tuner_service;

#[cfg(test)]
mod failure_injection_tests;

pub use demux_object::DemuxAidlObject;
pub use descrambler_object::DescramblerAidlObject;
pub use dvr_object::DvrAidlObject;
pub use filter_object::FilterAidlObject;
pub use frontend_object::FrontendAidlObject;
pub use lnb_object::LnbAidlObject;
pub use object_handle::{
    AidlObjectGeneration, AidlObjectHandle, AidlObjectHandleError, AidlObjectId, AidlObjectKind,
};
pub use service_context::{AidlServiceContext, SharedAidlServiceContext};
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
            AidlServiceContext::shared(
                maleicacid_tuner_hal2_service_runtime::TunerServiceRuntime::new(),
            ),
        )
        .unwrap();
        assert_eq!(frontend.handle().object_id(), AidlObjectId(1));
    }
}
