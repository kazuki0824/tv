pub mod descrambler_session;
pub mod frontend_capability;
pub mod fmq_queue;
pub mod stream_boundary;
pub mod registry_ledger;
pub mod lifecycle_txn;
pub mod worker_runtime;
pub mod hal_sync;
pub mod descrambler_key_table;
pub mod tuner_hal;

use crate::tuner_hal::TunerHal;
use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::ITuner::BnTuner;
use binder::BinderFeatures;
use maleicacid_tuner_hal_common::TUNER_SERVICE_NAME;

pub fn run_service() {
    binder::ProcessState::start_thread_pool();

    let tuner_binder = BnTuner::new_binder(TunerHal::new(), BinderFeatures::default());
    if let Err(e) = binder::add_service(TUNER_SERVICE_NAME, tuner_binder.as_binder()) {
        eprintln!("Tuner HAL service 登録に失敗しました {}: {:?}", TUNER_SERVICE_NAME, e);
        std::process::exit(1);
    }

    binder::ProcessState::join_thread_pool();
}
