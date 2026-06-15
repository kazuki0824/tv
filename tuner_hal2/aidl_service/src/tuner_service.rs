use std::sync::{Arc, Mutex, MutexGuard};

use android_hardware_common::aidl::android::hardware::common::NativeHandle::NativeHandle as CommonNativeHandle;
use android_hardware_common_fmq::aidl::android::hardware::common::fmq::MQDescriptor::MQDescriptor as CommonMqDescriptor;
use android_hardware_common_fmq::aidl::android::hardware::common::fmq::SynchronizedReadWrite::SynchronizedReadWrite as CommonSynchronizedReadWrite;
use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::{
    AvStreamType::AvStreamType,
    DemuxCapabilities::DemuxCapabilities,
    DemuxFilterMainType::DemuxFilterMainType,
    DemuxFilterSettings::DemuxFilterSettings,
    DemuxFilterType::DemuxFilterType,
    DemuxInfo::DemuxInfo,
    DemuxPid::DemuxPid,
    DvrSettings::DvrSettings,
    DvrType::DvrType,
    FilterDelayHint::FilterDelayHint,
    FrontendCapabilities::FrontendCapabilities,
    FrontendInfo::FrontendInfo,
    FrontendIsdbsCapabilities::FrontendIsdbsCapabilities,
    FrontendIsdbsCoderate::FrontendIsdbsCoderate,
    FrontendIsdbsModulation::FrontendIsdbsModulation,
    FrontendIsdbtBandwidth::FrontendIsdbtBandwidth,
    FrontendIsdbtCapabilities::FrontendIsdbtCapabilities,
    FrontendIsdbtCoderate::FrontendIsdbtCoderate,
    FrontendIsdbtGuardInterval::FrontendIsdbtGuardInterval,
    FrontendIsdbtMode::FrontendIsdbtMode,
    FrontendIsdbtModulation::FrontendIsdbtModulation,
    FrontendIsdbtTimeInterleaveMode::FrontendIsdbtTimeInterleaveMode,
    FrontendScanType::FrontendScanType,
    FrontendSettings::FrontendSettings,
    FrontendStatus::FrontendStatus,
    FrontendStatusReadiness::FrontendStatusReadiness,
    FrontendStatusType::FrontendStatusType,
    FrontendType::FrontendType,
    IDemux::{BnDemux, IDemux},
    IDescrambler::{BnDescrambler, IDescrambler},
    IDvr::IDvr,
    IDvrCallback::IDvrCallback,
    IFilter::{BnFilter, IFilter},
    IFilterCallback::IFilterCallback,
    IFrontend::{BnFrontend, IFrontend},
    IFrontendCallback::IFrontendCallback,
    ILnb::{BnLnb, ILnb},
    ILnbCallback::ILnbCallback,
    ITimeFilter::ITimeFilter,
    ITuner::ITuner,
    LnbPosition::LnbPosition,
    LnbTone::LnbTone,
    LnbVoltage::LnbVoltage,
    Result::Result as TunerResult,
};
use binder::binder_impl::Binder;
use binder::{BinderFeatures, Interface, Result as BinderResult, Status, Strong};
use maleicacid_tuner_hal2_binder_adapter::{
    aidl_frontend_settings_to_request, aidl_scan_type_to_mode, build_dvr_configure_request,
    build_dvr_open_request, build_filter_av_stream_type_request, build_filter_delay_hint_request,
    build_filter_summary_for_open_type, build_lnb_satellite_position_request,
    build_lnb_tone_request, build_lnb_voltage_request, build_open_filter_request, AidlApi,
    AidlFailureSource, AidlMethodAdapter, AidlMethodCall, AidlMethodPlan, AidlObjectGeneration,
    AidlObjectId, AidlObjectKind, AidlStatusMapper, DvrFilterLinkRequest,
    FilterReleaseAvHandleRequest, FilterSetDataSourceRequest, TunerStatusCode,
};
use maleicacid_tuner_hal2_common::{
    japan_isdbt_frequency_contract_range_hz, FrontendBackendKind, FrontendSystem, HalError,
    HalInvalidArgumentKind,
};
use maleicacid_tuner_hal2_device::{
    FrontendRuntimeState, FrontendSignalState, FrontendWorkerCancelReason, FrontendWorkerKind,
};
use maleicacid_tuner_hal2_service_runtime::{
    close_frontend_workers_and_live_data_use_case, start_frontend_scan_use_case,
    start_frontend_tune_use_case, stop_frontend_live_data_use_case,
    stop_frontend_scan_use_case, stop_frontend_tune_use_case, FrontendRegistryEntry,
    FrontendRuntimeId, LnbRegistryProfile, RuntimeCommandDispatchError,
    RuntimeCommandDispatchPlan, RuntimeOwnerRelation, TunerServiceRuntime,
};

use crate::child_object_open::{open_dvr_child_after_plan, open_filter_child_after_plan};
use crate::error_bridge::{service_error, status_from_hal_error, status_from_tuner_status, status_unknown_error};
use crate::demux_object::DemuxAidlObject;
use crate::descrambler_object::DescramblerAidlObject;
use crate::dvr_object::DvrAidlObject;
use crate::filter_object::FilterAidlObject;
use crate::frontend_callback_delivery::scan_end_notifier;
use crate::frontend_object::FrontendAidlObject;
use crate::lnb_object::LnbAidlObject;
use crate::object_handle::AidlObjectHandle;
use crate::object_runtime::{clear_live_lnb_callback_for_public_id, SharedTunerRuntime};

mod frontend_methods;
mod demux_methods;
mod filter_methods;
mod dvr_methods;
mod descrambler_methods;
mod lnb_methods;
mod support;

use self::support::{
    plan_tuner_public_api_method, runtime_entry_public_id, unavailable_after_tuner_method_plan,
};

type TunerQueueDesc = CommonMqDescriptor<i8, CommonSynchronizedReadWrite>;
type TunerNativeHandle = CommonNativeHandle;

const TUNER_HAL2_MAX_LIVE_DEMUXES: i32 = 8;
const TUNER_HAL2_DEMUX_MAX_TS_FILTERS: i32 = 32;
const TUNER_HAL2_DEMUX_MAX_SECTION_FILTERS: i32 = 8;
const TUNER_HAL2_DEMUX_MAX_AUDIO_FILTERS: i32 = 4;
const TUNER_HAL2_DEMUX_MAX_VIDEO_FILTERS: i32 = 4;
const TUNER_HAL2_DEMUX_MAX_PES_FILTERS: i32 = 8;
const TUNER_HAL2_MAX_SECTION_FILTER_BYTES: i64 = 16;
const DEMUX_FILTER_MAIN_TYPE_COUNT: usize = 5;
const SUPPORTED_DEMUX_FILTER_CAPS: i32 = DemuxFilterMainType::TS.0;

#[derive(Clone)]
pub struct TunerAidlService {
    runtime: Arc<Mutex<TunerServiceRuntime>>,
}

impl Interface for TunerAidlService {}

fn demux_link_caps_for_ts_filter_linkage() -> Vec<i32> {
    let mut link_caps = vec![0; DEMUX_FILTER_MAIN_TYPE_COUNT];
    link_caps[0] = DemuxFilterMainType::TS.0;
    link_caps
}

fn tuner_hal2_demux_capabilities() -> DemuxCapabilities {
    DemuxCapabilities {
        numDemux: TUNER_HAL2_MAX_LIVE_DEMUXES,
        numRecord: TUNER_HAL2_MAX_LIVE_DEMUXES,
        numPlayback: TUNER_HAL2_MAX_LIVE_DEMUXES,
        numTsFilter: TUNER_HAL2_DEMUX_MAX_TS_FILTERS,
        numSectionFilter: TUNER_HAL2_DEMUX_MAX_SECTION_FILTERS,
        numAudioFilter: TUNER_HAL2_DEMUX_MAX_AUDIO_FILTERS,
        numVideoFilter: TUNER_HAL2_DEMUX_MAX_VIDEO_FILTERS,
        numPesFilter: TUNER_HAL2_DEMUX_MAX_PES_FILTERS,
        numPcrFilter: 0,
        numBytesInSectionFilter: TUNER_HAL2_MAX_SECTION_FILTER_BYTES,
        filterCaps: SUPPORTED_DEMUX_FILTER_CAPS,
        linkCaps: demux_link_caps_for_ts_filter_linkage(),
        bTimeFilter: false,
    }
}

fn tuner_hal2_demux_info() -> DemuxInfo {
    DemuxInfo {
        filterTypes: SUPPORTED_DEMUX_FILTER_CAPS,
    }
}

impl TunerAidlService {
    pub fn new(runtime: TunerServiceRuntime) -> Self {
        Self {
            runtime: Arc::new(Mutex::new(runtime)),
        }
    }

    pub fn lock_runtime(&self) -> Result<MutexGuard<'_, TunerServiceRuntime>, Status> {
        self.runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))
    }

    fn frontend_entry(&self, frontend_id: i32) -> BinderResult<FrontendRegistryEntry> {
        self.lock_runtime()?
            .frontend_entry(frontend_id)
            .ok_or_else(|| {
                service_error(TunerResult::UNAVAILABLE.0, "frontend id is not available")
            })
    }

    pub fn plan_method(
        &self,
        method: AidlMethodCall,
    ) -> Result<RuntimeCommandDispatchPlan, RuntimeCommandDispatchError> {
        let method_plan = AidlMethodAdapter::plan(method);
        self.plan_from_method_plan(&method_plan)
    }

    pub fn plan_from_method_plan(
        &self,
        method_plan: &AidlMethodPlan,
    ) -> Result<RuntimeCommandDispatchPlan, RuntimeCommandDispatchError> {
        match self.runtime.lock() {
            Ok(mut runtime) => runtime.plan_command_dispatch(
                method_plan.command_plan,
                method_plan.command.runtime_executable_request(),
            ),
            Err(_) => Err(RuntimeCommandDispatchError::MissingDispatchTarget {
                transaction: method_plan.command_plan.transaction,
            }),
        }
    }

    fn register_child(
        &self,
        kind: AidlObjectKind,
        public_runtime_id: i64,
        owner: RuntimeOwnerRelation,
    ) -> BinderResult<AidlObjectHandle> {
        let entry = self
            .lock_runtime()?
            .register_aidl_object_for_runtime_auto_generation(kind, public_runtime_id, owner)
            .map_err(|_| {
                service_error(
                    TunerResult::INVALID_STATE.0,
                    "AIDL object registration failed",
                )
            })?;
        Ok(AidlObjectHandle::new(
            entry.object_kind,
            entry.object_id,
            entry.generation,
        ))
    }

    fn rollback_registered_child(&self, handle: AidlObjectHandle) -> BinderResult<()> {
        self.lock_runtime()?
            .unregister_aidl_object_after_registration_failure(
                handle.object_id(),
                handle.generation(),
            )
            .map(|_| ())
            .map_err(|_| {
                service_error(
                    TunerResult::UNKNOWN_ERROR.0,
                    "AIDL object registration rollback failed",
                )
            })
    }

    fn allocate_demux_runtime(&self) -> BinderResult<i32> {
        let entry = self
            .lock_runtime()?
            .allocate_demux_runtime()
            .map_err(|_| status_unknown_error("demux runtime allocation failed"))?;
        Ok(entry.id.0)
    }

    fn unregister_demux_runtime_after_open_failure(&self, id: i32) -> BinderResult<()> {
        self.lock_runtime()?.unregister_demux_runtime(id);
        Ok(())
    }

    fn allocate_descrambler_runtime(&self) -> BinderResult<i32> {
        let entry = self
            .lock_runtime()?
            .allocate_descrambler_runtime()
            .map_err(|_| status_unknown_error("descrambler runtime allocation failed"))?;
        Ok(entry.id.0)
    }

    fn unregister_descrambler_runtime_after_open_failure(&self, id: i32) -> BinderResult<()> {
        self.lock_runtime()?.unregister_descrambler_runtime(id);
        Ok(())
    }

    fn frontend_object(&self, id: i32) -> BinderResult<Strong<dyn IFrontend>> {
        let handle = self.register_child(
            AidlObjectKind::Frontend,
            i64::from(id),
            RuntimeOwnerRelation::Root,
        )?;
        match FrontendAidlObject::new(handle, self.runtime.clone()) {
            Ok(object) => Ok(BnFrontend::new_binder(object, BinderFeatures::default())),
            Err(_) => {
                self.rollback_registered_child(handle)?;
                Err(status_unknown_error("object kind mismatch"))
            }
        }
    }

    fn demux_object(&self, id: i32) -> BinderResult<Strong<dyn IDemux>> {
        let handle = self.register_child(
            AidlObjectKind::Demux,
            i64::from(id),
            RuntimeOwnerRelation::Root,
        )?;
        match DemuxAidlObject::new(handle, self.runtime.clone()) {
            Ok(object) => Ok(BnDemux::new_binder(object, BinderFeatures::default())),
            Err(_) => {
                self.rollback_registered_child(handle)?;
                Err(status_unknown_error("object kind mismatch"))
            }
        }
    }

    fn descrambler_object(&self, id: i32) -> BinderResult<Strong<dyn IDescrambler>> {
        let handle = self.register_child(
            AidlObjectKind::Descrambler,
            i64::from(id),
            RuntimeOwnerRelation::Root,
        )?;
        match DescramblerAidlObject::new(handle, self.runtime.clone()) {
            Ok(object) => Ok(BnDescrambler::new_binder(object, BinderFeatures::default())),
            Err(_) => {
                self.rollback_registered_child(handle)?;
                Err(status_unknown_error("object kind mismatch"))
            }
        }
    }

    fn lnb_object(&self, id: i32) -> BinderResult<Strong<dyn ILnb>> {
        let handle = self.register_child(
            AidlObjectKind::Lnb,
            i64::from(id),
            RuntimeOwnerRelation::Root,
        )?;
        let open_result = self
            .lock_runtime()?
            .open_lnb_for_public_id(id)
            .map_err(status_from_hal_error);
        if let Err(status) = open_result {
            self.rollback_registered_child(handle)?;
            return Err(status);
        }
        match LnbAidlObject::new(handle, self.runtime.clone()) {
            Ok(object) => Ok(BnLnb::new_binder(object, BinderFeatures::default())),
            Err(_) => {
                self.rollback_registered_child(handle)?;
                Err(status_unknown_error("object kind mismatch"))
            }
        }
    }
}

const JAPAN_BS_FIRST_IF_HZ: i64 = 1_049_480_000;
const JAPAN_CS110_LAST_IF_HZ: i64 = 2_053_000_000;
const PX4_FRONTEND_ID_BASE: i32 = 1_000_000;
const DVB_FRONTEND_ID_BASE: i32 = 2_000_000;
const PX4_PHYSICAL_GROUP_TAG: i32 = 0x1000_0000;
const DVB_PHYSICAL_GROUP_TAG: i32 = 0x2000_0000;

fn packed_physical_group_id(tag: i32, major: i32, minor: i32) -> i32 {
    let major_bits = (major.max(0) & 0x3fff) << 14;
    let minor_bits = minor.max(0) & 0x3fff;
    tag | major_bits | minor_bits
}

fn frontend_type_from_entry(entry: &FrontendRegistryEntry) -> FrontendType {
    match entry.system {
        FrontendSystem::IsdbT => FrontendType::ISDBT,
        FrontendSystem::IsdbS => FrontendType::ISDBS,
        FrontendSystem::IsdbS3 | FrontendSystem::DvbS => FrontendType::ISDBT,
    }
}

fn physical_group_id_from_entry(entry: &FrontendRegistryEntry) -> i32 {
    match entry.backend {
        FrontendBackendKind::LinuxDvb => {
            let rel = entry.id.0.saturating_sub(DVB_FRONTEND_ID_BASE);
            let adapter = (rel >> 12) & 0xff;
            let frontend_index = (rel >> 4) & 0xff;
            packed_physical_group_id(DVB_PHYSICAL_GROUP_TAG, adapter, frontend_index)
        }
        FrontendBackendKind::Px4CharDevice => {
            let rel = entry.id.0.saturating_sub(PX4_FRONTEND_ID_BASE);
            let family = rel.div_euclid(10_000);
            let unit = rel.rem_euclid(10_000).div_euclid(10);
            packed_physical_group_id(PX4_PHYSICAL_GROUP_TAG, family, unit)
        }
    }
}

fn lnb_profile_supports_voltage_status(profile: Option<LnbRegistryProfile>) -> bool {
    matches!(
        profile,
        Some(LnbRegistryProfile::Px4Device15VOnly | LnbRegistryProfile::EarthPt1FixedLnb)
    )
}

fn lnb_profile_status_voltage(profile: Option<LnbRegistryProfile>) -> LnbVoltage {
    match profile {
        Some(LnbRegistryProfile::Px4Device15VOnly | LnbRegistryProfile::EarthPt1FixedLnb) => {
            LnbVoltage::NONE
        }
        Some(LnbRegistryProfile::NoPower) | None => LnbVoltage::NONE,
    }
}

fn frontend_status_caps_for_entry(entry: &FrontendRegistryEntry) -> Vec<FrontendStatusType> {
    // optional telemetryは保守的に扱う。tune/scan backend runtime接続前は決定的な状態fieldだけをadvertiseする。
    // LNB voltageは、systemがISDB-Sであるだけではなく、frontend exportとexported LNBがprobe/registry由来の同じ固定LNB profileを共有する場合だけadvertiseする。
    let mut caps = vec![FrontendStatusType::DEMOD_LOCK];
    if lnb_profile_supports_voltage_status(entry.lnb_profile) {
        caps.push(FrontendStatusType::LNB_VOLTAGE);
    }
    caps
}

fn is_supported_frontend_status(
    entry: &FrontendRegistryEntry,
    status_type: FrontendStatusType,
) -> bool {
    frontend_status_caps_for_entry(entry).contains(&status_type)
}

fn frontend_status_for_types(
    entry: &FrontendRegistryEntry,
    signal_state: FrontendSignalState,
    status_types: &[FrontendStatusType],
) -> BinderResult<Vec<FrontendStatus>> {
    if status_types
        .iter()
        .any(|ty| !is_supported_frontend_status(entry, *ty))
    {
        return Err(service_error(
            TunerResult::INVALID_ARGUMENT.0,
            "unsupported frontend status type requested",
        ));
    }
    Ok(status_types
        .iter()
        .map(|ty| match *ty {
            FrontendStatusType::DEMOD_LOCK => {
                FrontendStatus::IsDemodLocked(matches!(signal_state, FrontendSignalState::Locked))
            }
            FrontendStatusType::LNB_VOLTAGE => {
                FrontendStatus::LnbVoltage(lnb_profile_status_voltage(entry.lnb_profile))
            }
            _ => FrontendStatus::IsDemodLocked(false),
        })
        .collect())
}

fn frontend_readiness_for_types(
    entry: &FrontendRegistryEntry,
    runtime_state: FrontendRuntimeState,
    signal_state: FrontendSignalState,
    status_types: &[FrontendStatusType],
) -> Vec<FrontendStatusReadiness> {
    status_types
        .iter()
        .map(|ty| {
            if !is_supported_frontend_status(entry, *ty) {
                return FrontendStatusReadiness::UNSUPPORTED;
            }
            match runtime_state {
                FrontendRuntimeState::Idle => FrontendStatusReadiness::STABLE,
                FrontendRuntimeState::Tuning { .. } | FrontendRuntimeState::Scanning { .. } => {
                    match signal_state {
                        FrontendSignalState::Locked => FrontendStatusReadiness::STABLE,
                        FrontendSignalState::NoSignal
                        | FrontendSignalState::SignalDetected
                        | FrontendSignalState::Unknown => FrontendStatusReadiness::UNSTABLE,
                    }
                }
                FrontendRuntimeState::Closing | FrontendRuntimeState::Failed => {
                    FrontendStatusReadiness::UNAVAILABLE
                }
            }
        })
        .collect()
}

fn frontend_runtime_state_for_object(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
) -> BinderResult<FrontendRuntimeState> {
    let entry = runtime_frontend_entry_for_object(runtime, handle)?;
    let guard = runtime
        .lock()
        .map_err(|_| status_unknown_error("service runtime lock poisoned"))?;
    guard
        .registry()
        .frontend_runtime(FrontendRuntimeId(entry.id.0))
        .map(|frontend| frontend.state())
        .ok_or_else(|| status_unknown_error("frontend runtime is missing for advertised frontend"))
}

fn frontend_signal_state_for_object(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
) -> BinderResult<FrontendSignalState> {
    let frontend_id = runtime_entry_public_id(runtime, handle, AidlObjectKind::Frontend)?;
    let guard = runtime
        .lock()
        .map_err(|_| status_unknown_error("service runtime lock poisoned"))?;
    guard
        .frontend_signal_state(frontend_id)
        .map_err(status_from_hal_error)
}

fn isdbt_mode_caps() -> i32 {
    FrontendIsdbtMode::AUTO.0 | FrontendIsdbtMode::MODE_3.0
}
fn isdbt_bandwidth_caps() -> i32 {
    FrontendIsdbtBandwidth::AUTO.0 | FrontendIsdbtBandwidth::BANDWIDTH_6MHZ.0
}
fn isdbt_modulation_caps() -> i32 {
    FrontendIsdbtModulation::AUTO.0
        | FrontendIsdbtModulation::MOD_DQPSK.0
        | FrontendIsdbtModulation::MOD_QPSK.0
        | FrontendIsdbtModulation::MOD_16QAM.0
        | FrontendIsdbtModulation::MOD_64QAM.0
}
fn isdbt_coderate_caps() -> i32 {
    FrontendIsdbtCoderate::AUTO.0
        | FrontendIsdbtCoderate::CODERATE_1_2.0
        | FrontendIsdbtCoderate::CODERATE_2_3.0
        | FrontendIsdbtCoderate::CODERATE_3_4.0
        | FrontendIsdbtCoderate::CODERATE_5_6.0
        | FrontendIsdbtCoderate::CODERATE_7_8.0
}
fn isdbt_guard_interval_caps() -> i32 {
    FrontendIsdbtGuardInterval::AUTO.0
        | FrontendIsdbtGuardInterval::INTERVAL_1_32.0
        | FrontendIsdbtGuardInterval::INTERVAL_1_16.0
        | FrontendIsdbtGuardInterval::INTERVAL_1_8.0
        | FrontendIsdbtGuardInterval::INTERVAL_1_4.0
}
fn isdbt_time_interleave_caps() -> i32 {
    FrontendIsdbtTimeInterleaveMode::AUTO.0
        | FrontendIsdbtTimeInterleaveMode::INTERLEAVE_3_0.0
        | FrontendIsdbtTimeInterleaveMode::INTERLEAVE_3_1.0
        | FrontendIsdbtTimeInterleaveMode::INTERLEAVE_3_2.0
        | FrontendIsdbtTimeInterleaveMode::INTERLEAVE_3_4.0
}
fn isdbs_modulation_caps() -> i32 {
    FrontendIsdbsModulation::AUTO.0
        | FrontendIsdbsModulation::MOD_BPSK.0
        | FrontendIsdbsModulation::MOD_QPSK.0
        | FrontendIsdbsModulation::MOD_TC8PSK.0
}
fn isdbs_coderate_caps() -> i32 {
    FrontendIsdbsCoderate::AUTO.0
        | FrontendIsdbsCoderate::CODERATE_1_2.0
        | FrontendIsdbsCoderate::CODERATE_2_3.0
        | FrontendIsdbsCoderate::CODERATE_3_4.0
        | FrontendIsdbsCoderate::CODERATE_5_6.0
        | FrontendIsdbsCoderate::CODERATE_7_8.0
}

fn frontend_caps_for_entry(entry: &FrontendRegistryEntry) -> FrontendCapabilities {
    match frontend_type_from_entry(entry) {
        FrontendType::ISDBT => FrontendCapabilities::IsdbtCaps(FrontendIsdbtCapabilities {
            modeCap: isdbt_mode_caps(),
            bandwidthCap: isdbt_bandwidth_caps(),
            modulationCap: isdbt_modulation_caps(),
            coderateCap: isdbt_coderate_caps(),
            guardIntervalCap: isdbt_guard_interval_caps(),
            timeInterleaveCap: isdbt_time_interleave_caps(),
            isSegmentAuto: true,
            isFullSegment: true,
        }),
        FrontendType::ISDBS => FrontendCapabilities::IsdbsCaps(FrontendIsdbsCapabilities {
            modulationCap: isdbs_modulation_caps(),
            coderateCap: isdbs_coderate_caps(),
        }),
        _ => Default::default(),
    }
}

fn frontend_frequency_contract(entry: &FrontendRegistryEntry) -> (i64, i64, i64) {
    match frontend_type_from_entry(entry) {
        FrontendType::ISDBT => {
            let (min_hz, max_hz, tolerance_hz) = japan_isdbt_frequency_contract_range_hz();
            (min_hz as i64, max_hz as i64, tolerance_hz as i64)
        }
        FrontendType::ISDBS => (JAPAN_BS_FIRST_IF_HZ, JAPAN_CS110_LAST_IF_HZ, 0),
        _ => (0, 0, 0),
    }
}

fn frontend_info_from_entry(entry: &FrontendRegistryEntry) -> FrontendInfo {
    let (min_freq, max_freq, acquire_range) = frontend_frequency_contract(entry);
    FrontendInfo {
        r#type: frontend_type_from_entry(entry),
        minFrequency: min_freq,
        maxFrequency: max_freq,
        minSymbolRate: 0,
        maxSymbolRate: 0,
        acquireRange: acquire_range,
        exclusiveGroupId: physical_group_id_from_entry(entry),
        statusCaps: frontend_status_caps_for_entry(entry),
        frontendCaps: frontend_caps_for_entry(entry),
    }
}

fn runtime_frontend_entry_for_object(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
) -> BinderResult<FrontendRegistryEntry> {
    let frontend_id = runtime_entry_public_id(runtime, handle, AidlObjectKind::Frontend)?;
    runtime
        .lock()
        .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
        .frontend_entry(frontend_id)
        .ok_or_else(|| {
            service_error(
                TunerResult::UNAVAILABLE.0,
                "frontend runtime entry is not available",
            )
        })
}

impl ITuner for TunerAidlService {
    fn getFrontendIds(&self) -> BinderResult<Vec<i32>> {
        plan_tuner_public_api_method(self, AidlApi::TunerGetFrontendIds, None)?;
        let runtime = self.lock_runtime()?;
        Ok(runtime.frontend_ids())
    }

    fn openFrontendById(&self, frontend_id: i32) -> BinderResult<Strong<dyn IFrontend>> {
        plan_tuner_public_api_method(self, AidlApi::TunerOpenFrontendById, None)?;
        {
            let runtime = self.lock_runtime()?;
            if !runtime.has_frontend_id(frontend_id) {
                return Err(status_from_hal_error(HalError::Unsupported(
                    "frontend id is not available",
                )));
            }
        }
        self.frontend_object(frontend_id)
    }

    fn openDemux(&self, demux_id: &mut Vec<i32>) -> BinderResult<Strong<dyn IDemux>> {
        demux_id.clear();
        plan_tuner_public_api_method(self, AidlApi::TunerOpenDemux, None)?;
        let id = self.allocate_demux_runtime()?;
        match self.demux_object(id) {
            Ok(object) => {
                demux_id.push(id);
                Ok(object)
            }
            Err(status) => {
                self.unregister_demux_runtime_after_open_failure(id)?;
                Err(status)
            }
        }
    }

    fn getDemuxCaps(&self) -> BinderResult<DemuxCapabilities> {
        plan_tuner_public_api_method(self, AidlApi::TunerGetDemuxCaps, None)?;
        Ok(tuner_hal2_demux_capabilities())
    }

    fn openDescrambler(&self) -> BinderResult<Strong<dyn IDescrambler>> {
        plan_tuner_public_api_method(self, AidlApi::TunerOpenDescrambler, None)?;
        let id = self.allocate_descrambler_runtime()?;
        match self.descrambler_object(id) {
            Ok(object) => Ok(object),
            Err(status) => {
                self.unregister_descrambler_runtime_after_open_failure(id)?;
                Err(status)
            }
        }
    }

    fn getFrontendInfo(&self, frontend_id: i32) -> BinderResult<FrontendInfo> {
        plan_tuner_public_api_method(self, AidlApi::TunerGetFrontendInfo, None)?;
        let entry = self.frontend_entry(frontend_id)?;
        Ok(frontend_info_from_entry(&entry))
    }

    fn getLnbIds(&self) -> BinderResult<Vec<i32>> {
        plan_tuner_public_api_method(self, AidlApi::TunerGetLnbIds, None)?;
        let runtime = self.lock_runtime()?;
        Ok(runtime.lnb_ids())
    }

    fn openLnbById(&self, lnb_id: i32) -> BinderResult<Strong<dyn ILnb>> {
        plan_tuner_public_api_method(self, AidlApi::TunerOpenLnbById, None)?;
        let runtime = self.lock_runtime()?;
        if !runtime.has_lnb_id(lnb_id) {
            return Err(status_from_hal_error(HalError::Unsupported(
                "LNB id is not available",
            )));
        }
        drop(runtime);
        self.lnb_object(lnb_id)
    }

    fn openLnbByName(
        &self,
        lnb_name: &str,
        lnb_id: &mut Vec<i32>,
    ) -> BinderResult<Strong<dyn ILnb>> {
        lnb_id.clear();
        plan_tuner_public_api_method(self, AidlApi::TunerOpenLnbByName, None)?;
        let id = {
            let runtime = self.lock_runtime()?;
            runtime.lnb_id_by_name(lnb_name)
        };
        match id {
            Some(id) => {
                let object = self.lnb_object(id)?;
                lnb_id.push(id);
                Ok(object)
            }
            None => Err(status_from_hal_error(HalError::Unsupported(
                "LNB name is not available",
            ))),
        }
    }

    fn setLna(&self, _b_enable: bool) -> BinderResult<()> {
        unavailable_after_tuner_method_plan(self, AidlApi::TunerSetLna, None, "LNA is unsupported")
    }

    fn setMaxNumberOfFrontends(
        &self,
        _frontend_type: FrontendType,
        max_number: i32,
    ) -> BinderResult<()> {
        let input = None;
        if max_number == 0 {
            plan_tuner_public_api_method(self, AidlApi::TunerSetMaxNumberOfFrontends, input)?;
            Ok(())
        } else {
            unavailable_after_tuner_method_plan(
                self,
                AidlApi::TunerSetMaxNumberOfFrontends,
                input,
                "frontend max override is unavailable without probed frontend",
            )
        }
    }

    fn getMaxNumberOfFrontends(&self, _frontend_type: FrontendType) -> BinderResult<i32> {
        plan_tuner_public_api_method(self, AidlApi::TunerGetMaxNumberOfFrontends, None)?;
        Ok(0)
    }

    fn isLnaSupported(&self) -> BinderResult<bool> {
        plan_tuner_public_api_method(self, AidlApi::TunerIsLnaSupported, None)?;
        Ok(false)
    }

    fn getDemuxIds(&self) -> BinderResult<Vec<i32>> {
        plan_tuner_public_api_method(self, AidlApi::TunerGetDemuxIds, None)?;
        let runtime = self.lock_runtime()?;
        Ok(runtime.demux_ids())
    }

    fn openDemuxById(&self, demux_id: i32) -> BinderResult<Strong<dyn IDemux>> {
        plan_tuner_public_api_method(self, AidlApi::TunerOpenDemuxById, None)?;
        let runtime = self.lock_runtime()?;
        if !runtime.has_demux_id(demux_id) {
            return Err(status_from_hal_error(HalError::Unsupported(
                "demux id is not available",
            )));
        }
        drop(runtime);
        self.demux_object(demux_id)
    }

    fn getDemuxInfo(&self, demux_id: i32) -> BinderResult<DemuxInfo> {
        plan_tuner_public_api_method(self, AidlApi::TunerGetDemuxInfo, None)?;
        let runtime = self.lock_runtime()?;
        if !runtime.has_demux_id(demux_id) {
            return Err(status_from_hal_error(HalError::Unsupported(
                "demux id is not available",
            )));
        }
        Ok(tuner_hal2_demux_info())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use maleicacid_tuner_hal2_common::{FrontendSystem, FrontendTuneRequest};

    #[test]
    fn demux_capabilities_advertise_ts_only_profile() {
        let caps = tuner_hal2_demux_capabilities();

        assert_eq!(caps.numDemux, TUNER_HAL2_MAX_LIVE_DEMUXES);
        assert_eq!(caps.numRecord, TUNER_HAL2_MAX_LIVE_DEMUXES);
        assert_eq!(caps.numPlayback, TUNER_HAL2_MAX_LIVE_DEMUXES);
        assert_eq!(caps.numTsFilter, 32);
        assert_eq!(caps.numSectionFilter, 8);
        assert_eq!(caps.numAudioFilter, 4);
        assert_eq!(caps.numVideoFilter, 4);
        assert_eq!(caps.numPesFilter, 8);
        assert_eq!(caps.numBytesInSectionFilter, 16);
        assert_eq!(caps.filterCaps, DemuxFilterMainType::TS.0);
        assert_eq!(caps.linkCaps, vec![DemuxFilterMainType::TS.0, 0, 0, 0, 0]);
        assert!(!caps.bTimeFilter);
    }

    #[test]
    fn demux_info_advertises_same_ts_only_filter_mask() {
        assert_eq!(
            tuner_hal2_demux_info().filterTypes,
            DemuxFilterMainType::TS.0
        );
    }

    #[test]
    fn configure_ip_cid_returns_unavailable_for_any_value() {
        let service = TunerAidlService::new(TunerServiceRuntime::new());
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Filter,
            AidlObjectId(10),
            AidlObjectGeneration(1),
        );
        {
            let mut runtime = service.lock_runtime().unwrap();
            runtime
                .register_aidl_object_for_runtime(
                    AidlObjectKind::Filter,
                    AidlObjectId(10),
                    AidlObjectGeneration(1),
                    10,
                    RuntimeOwnerRelation::Root,
                )
                .unwrap();
        }
        let filter = FilterAidlObject::new(handle, service.runtime.clone()).unwrap();
        assert!(filter.configureIpCid(-1).is_err());
    }

    #[test]
    fn configure_monitor_event_zero_succeeds_nonzero_unavailable() {
        let service = TunerAidlService::new(TunerServiceRuntime::new());
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Filter,
            AidlObjectId(11),
            AidlObjectGeneration(1),
        );
        {
            let mut runtime = service.lock_runtime().unwrap();
            runtime
                .register_aidl_object_for_runtime(
                    AidlObjectKind::Filter,
                    AidlObjectId(11),
                    AidlObjectGeneration(1),
                    11,
                    RuntimeOwnerRelation::Root,
                )
                .unwrap();
        }
        let filter = FilterAidlObject::new(handle, service.runtime.clone()).unwrap();
        assert!(filter.configureMonitorEvent(0).is_ok());
        assert!(filter.configureMonitorEvent(1).is_err());
    }

    #[test]
    fn service_plans_aidl_method_without_string_dispatch() {
        let service = TunerAidlService::new(TunerServiceRuntime::new());
        let request = FrontendTuneRequest {
            system: FrontendSystem::IsdbT,
            frequency: 473_143_000,
            end_frequency: None,
            stream_id: None,
            stream_id_kind: None,
            bandwidth_hz: None,
            symbol_rate: None,
        };
        let plan = service
            .plan_method(AidlMethodCall::FrontendTune(request))
            .unwrap();
        assert_eq!(
            plan.command_plan.transaction,
            maleicacid_tuner_hal2_binder_adapter::RuntimeTransactionName::FrontendTuneTxnApply
        );
    }

    #[test]
    fn frontend_close_requests_tune_and_scan_worker_stop() {
        let service = TunerAidlService::new(TunerServiceRuntime::new());
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Frontend,
            AidlObjectId(20),
            AidlObjectGeneration(1),
        );
        let (reason_tx, reason_rx) = std::sync::mpsc::channel();
        {
            let mut runtime = service.lock_runtime().unwrap();
            runtime
                .register_aidl_object_for_runtime(
                    AidlObjectKind::Frontend,
                    AidlObjectId(20),
                    AidlObjectGeneration(1),
                    7,
                    RuntimeOwnerRelation::Root,
                )
                .unwrap();
            let tx = reason_tx.clone();
            runtime
                .start_frontend_worker(7, FrontendWorkerKind::Tune, 1, move |ctx| {
                    while !ctx.cancel_requested() {
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                    tx.send((ctx.kind(), ctx.cancel_reason().unwrap())).unwrap();
                    Ok(())
                })
                .unwrap();
            runtime
                .start_frontend_worker(7, FrontendWorkerKind::Scan, 1, move |ctx| {
                    while !ctx.cancel_requested() {
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                    reason_tx.send((ctx.kind(), ctx.cancel_reason().unwrap())).unwrap();
                    Ok(())
                })
                .unwrap();
        }
        let frontend = FrontendAidlObject::new(handle, service.runtime.clone()).unwrap();
        assert!(frontend.close().is_ok());
        let first = reason_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        let second = reason_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        let mut reasons = vec![first, second];
        reasons.sort_by_key(|(kind, _)| *kind);
        assert_eq!(
            reasons,
            vec![
                (
                    FrontendWorkerKind::Tune,
                    Some(FrontendWorkerCancelReason::FrontendClosing)
                ),
                (
                    FrontendWorkerKind::Scan,
                    Some(FrontendWorkerCancelReason::FrontendClosing)
                ),
            ]
        );
    }

    #[test]
    fn frontend_readiness_uses_runtime_and_backend_signal_state() {
        let entry = FrontendRegistryEntry {
            id: FrontendRuntimeId(2_000_001),
            backend: FrontendBackendKind::LinuxDvb,
            system: FrontendSystem::IsdbS,
            device_path: std::path::PathBuf::from("/dev/dvb/adapter0/frontend0"),
            lnb_profile: Some(LnbRegistryProfile::EarthPt1FixedLnb),
        };
        let status_types = vec![
            FrontendStatusType::DEMOD_LOCK,
            FrontendStatusType::LNB_VOLTAGE,
        ];

        assert_eq!(
            frontend_readiness_for_types(
                &entry,
                FrontendRuntimeState::Tuning { generation: 1 },
                FrontendSignalState::Locked,
                &status_types,
            ),
            vec![
                FrontendStatusReadiness::STABLE,
                FrontendStatusReadiness::STABLE
            ]
        );
        assert_eq!(
            frontend_readiness_for_types(
                &entry,
                FrontendRuntimeState::Scanning { generation: 2 },
                FrontendSignalState::Unknown,
                &status_types,
            ),
            vec![
                FrontendStatusReadiness::UNSTABLE,
                FrontendStatusReadiness::UNSTABLE
            ]
        );
        assert_eq!(
            frontend_readiness_for_types(
                &entry,
                FrontendRuntimeState::Failed,
                FrontendSignalState::NoSignal,
                &status_types,
            ),
            vec![
                FrontendStatusReadiness::UNAVAILABLE,
                FrontendStatusReadiness::UNAVAILABLE
            ]
        );
    }
}
