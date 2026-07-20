pub mod descrambler_key_table;
pub mod descrambler_session;
pub mod fmq_queue;
pub mod frontend_capability;
pub mod hal_sync;
pub mod lifecycle_txn;
pub mod queue_cleanup_txn;
pub mod registry_ledger;
pub mod stream_boundary;
pub mod tuner_hal;
pub mod worker_runtime;

use crate::tuner_hal::TunerHal;
use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::ITuner::BnTuner;
use binder::BinderFeatures;
use maleicacid_tuner_hal_common::TUNER_SERVICE_NAME;

pub fn run_service() {
    binder::ProcessState::start_thread_pool();

    let tuner_hal = match TunerHal::new() {
        Ok(tuner_hal) => tuner_hal,
        Err(e) => {
            eprintln!(
                "Tuner HAL service 初期化に失敗しました {}: {:?}",
                TUNER_SERVICE_NAME, e
            );
            std::process::exit(1);
        }
    };
    let tuner_binder = BnTuner::new_binder(tuner_hal, BinderFeatures::default());
    if let Err(e) = binder::add_service(TUNER_SERVICE_NAME, tuner_binder.as_binder()) {
        eprintln!(
            "Tuner HAL service 登録に失敗しました {}: {:?}",
            TUNER_SERVICE_NAME, e
        );
        std::process::exit(1);
    }

    binder::ProcessState::join_thread_pool();
}
