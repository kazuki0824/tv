use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::DemuxPid::DemuxPid;
use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::IDescrambler::IDescrambler;
use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::IFilter::IFilter;
use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::ILnb::ILnb;
use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::ILnbCallback::ILnbCallback;
use binder::{Result as BinderResult, Strong};

#[allow(dead_code)]
fn assert_filter_nullable_signature<T: IFilter>() {
    let _: fn(&T, Option<&Strong<dyn IFilter>>) -> BinderResult<()> = <T as IFilter>::setDataSource;
}

#[allow(dead_code)]
fn assert_descrambler_nullable_signatures<T: IDescrambler>() {
    let _: fn(&T, &DemuxPid, Option<&Strong<dyn IFilter>>) -> BinderResult<()> =
        <T as IDescrambler>::addPid;
    let _: fn(&T, &DemuxPid, Option<&Strong<dyn IFilter>>) -> BinderResult<()> =
        <T as IDescrambler>::removePid;
}

#[allow(dead_code)]
fn assert_lnb_nullable_signature<T: ILnb>() {
    let _: fn(&T, Option<&Strong<dyn ILnbCallback>>) -> BinderResult<()> = <T as ILnb>::setCallback;
}

#[test]
fn nullable_tuner_aidl_v3_signatures_are_type_checked() {
    // 上記の検査はコンパイル時契約である。この実行時テストにより、
    // 通常のAndroidテスト一覧にこのクレートを含める。
}
