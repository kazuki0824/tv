use std::ffi::CString;
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
use maleicacid_tuner_hal2_service_runtime::frontend_worker_txn::{
    close_frontend_workers_and_live_data, start_frontend_backend_scan_session_worker,
    start_frontend_backend_tune_worker, stop_frontend_live_data_and_unbind,
    stop_frontend_scan_worker, stop_frontend_tune_worker,
};
use maleicacid_tuner_hal2_service_runtime::{
    FrontendRegistryEntry, FrontendRuntimeId, LnbRegistryProfile, RuntimeCommandDispatchError,
    RuntimeCommandDispatchPlan, RuntimeOwnerRelation, TunerServiceRuntime,
};

use crate::child_object_open::{open_dvr_child_after_plan, open_filter_child_after_plan};
use crate::demux_object::DemuxAidlObject;
use crate::descrambler_object::DescramblerAidlObject;
use crate::dvr_object::DvrAidlObject;
use crate::filter_object::FilterAidlObject;
use crate::frontend_callback_delivery::scan_end_notifier;
use crate::frontend_object::FrontendAidlObject;
use crate::lnb_object::LnbAidlObject;
use crate::object_handle::AidlObjectHandle;
use crate::object_runtime::{clear_live_lnb_callback_for_public_id, SharedTunerRuntime};

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
        plan_tuner_query_method(self, AidlApi::TunerGetFrontendIds, None)?;
        let runtime = self.lock_runtime()?;
        Ok(runtime.frontend_ids())
    }

    fn openFrontendById(&self, frontend_id: i32) -> BinderResult<Strong<dyn IFrontend>> {
        plan_tuner_query_method(self, AidlApi::TunerOpenFrontendById, None)?;
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
        plan_tuner_query_method(self, AidlApi::TunerOpenDemux, None)?;
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
        plan_tuner_query_method(self, AidlApi::TunerGetDemuxCaps, None)?;
        Ok(tuner_hal2_demux_capabilities())
    }

    fn openDescrambler(&self) -> BinderResult<Strong<dyn IDescrambler>> {
        plan_tuner_query_method(self, AidlApi::TunerOpenDescrambler, None)?;
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
        plan_tuner_query_method(self, AidlApi::TunerGetFrontendInfo, None)?;
        let entry = self.frontend_entry(frontend_id)?;
        Ok(frontend_info_from_entry(&entry))
    }

    fn getLnbIds(&self) -> BinderResult<Vec<i32>> {
        plan_tuner_query_method(self, AidlApi::TunerGetLnbIds, None)?;
        let runtime = self.lock_runtime()?;
        Ok(runtime.lnb_ids())
    }

    fn openLnbById(&self, lnb_id: i32) -> BinderResult<Strong<dyn ILnb>> {
        plan_tuner_query_method(self, AidlApi::TunerOpenLnbById, None)?;
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
        plan_tuner_query_method(self, AidlApi::TunerOpenLnbByName, None)?;
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
            plan_tuner_query_method(self, AidlApi::TunerSetMaxNumberOfFrontends, input)?;
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
        plan_tuner_query_method(self, AidlApi::TunerGetMaxNumberOfFrontends, None)?;
        Ok(0)
    }

    fn isLnaSupported(&self) -> BinderResult<bool> {
        plan_tuner_query_method(self, AidlApi::TunerIsLnaSupported, None)?;
        Ok(false)
    }

    fn getDemuxIds(&self) -> BinderResult<Vec<i32>> {
        plan_tuner_query_method(self, AidlApi::TunerGetDemuxIds, None)?;
        let runtime = self.lock_runtime()?;
        Ok(runtime.demux_ids())
    }

    fn openDemuxById(&self, demux_id: i32) -> BinderResult<Strong<dyn IDemux>> {
        plan_tuner_query_method(self, AidlApi::TunerOpenDemuxById, None)?;
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
        plan_tuner_query_method(self, AidlApi::TunerGetDemuxInfo, None)?;
        let runtime = self.lock_runtime()?;
        if !runtime.has_demux_id(demux_id) {
            return Err(status_from_hal_error(HalError::Unsupported(
                "demux id is not available",
            )));
        }
        Ok(tuner_hal2_demux_info())
    }
}

fn ts_pid_from_demux_pid(pid: &DemuxPid) -> Result<u16, HalError> {
    match pid {
        DemuxPid::TPid(value) if (0..=0x1ffe).contains(value) => Ok(*value as u16),
        DemuxPid::TPid(_) => Err(HalError::invalid_argument(
            HalInvalidArgumentKind::NumericRange,
            "TS PID must be in 0..=0x1ffe for descrambler binding",
        )),
        _ => Err(HalError::Unsupported(
            "non-TS descrambler PID is outside the r51 TS-only profile",
        )),
    }
}

fn unavailable_after_method_plan(
    plan: BinderResult<AidlMethodPlan>,
    message: &'static str,
) -> BinderResult<()> {
    let plan = plan?;
    let failure = AidlFailureSource::RuntimeDispatch(HalError::Unsupported(message));
    let failures = [failure];
    let status =
        AidlStatusMapper::resolve_failure_by_precedence(plan.command_plan.api, &failures, false)
            .unwrap_or(TunerStatusCode::UnknownError);
    Err(status_from_tuner_status(status, message))
}

fn unsupported_public_api_call(
    object: AidlObjectKind,
    api: AidlApi,
    _input: Option<()>,
) -> AidlMethodCall {
    AidlMethodCall::UnsupportedPublicApi { object, api }
}

fn unavailable_after_tuner_method_plan(
    service: &TunerAidlService,
    api: AidlApi,
    input: Option<()>,
    message: &'static str,
) -> BinderResult<()> {
    let method = unsupported_public_api_call(AidlObjectKind::Tuner, api, input);
    let method_plan = AidlMethodAdapter::plan(method);
    service
        .plan_from_method_plan(&method_plan)
        .map_err(|err| status_from_hal_error(err.into_hal_error()))?;
    let failure = AidlFailureSource::RuntimeDispatch(HalError::Unsupported(message));
    let failures = [failure];
    let status = AidlStatusMapper::resolve_failure_by_precedence(
        method_plan.command_plan.api,
        &failures,
        false,
    )
    .unwrap_or(TunerStatusCode::UnknownError);
    Err(status_from_tuner_status(status, message))
}

fn plan_tuner_query_method(
    service: &TunerAidlService,
    api: AidlApi,
    input: Option<()>,
) -> BinderResult<()> {
    let method = unsupported_public_api_call(AidlObjectKind::Tuner, api, input);
    let method_plan = AidlMethodAdapter::plan(method);
    service
        .plan_from_method_plan(&method_plan)
        .map_err(|err| status_from_hal_error(err.into_hal_error()))?;
    Ok(())
}

fn unavailable_after_object_public_api_plan(
    plan: BinderResult<AidlMethodPlan>,
    message: &'static str,
) -> BinderResult<()> {
    unavailable_after_method_plan(plan, message)
}

fn runtime_entry_public_id(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    expected_kind: AidlObjectKind,
) -> BinderResult<i32> {
    let runtime = runtime
        .lock()
        .map_err(|_| status_unknown_error("service runtime lock poisoned"))?;
    let entry = runtime
        .object_table()
        .entry_for_kind(handle.object_id(), handle.generation(), expected_kind)
        .map_err(|_| service_error(TunerResult::INVALID_STATE.0, "AIDL object lookup failed"))?;
    i32::try_from(entry.ledger_id.0)
        .map_err(|_| status_unknown_error("public runtime id out of i32 range"))
}

fn current_filter_open_type(
    filter: &FilterAidlObject,
) -> BinderResult<maleicacid_tuner_hal2_demux::FilterOpenType> {
    let runtime = filter.runtime();
    let public_id = runtime_entry_public_id(&runtime, filter.handle(), AidlObjectKind::Filter)?;
    let open_type = runtime
        .lock()
        .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
        .filter_open_type(public_id)
        .ok_or_else(|| service_error(TunerResult::INVALID_STATE.0, "filter runtime is missing"))?;
    Ok(open_type)
}

fn local_filter_handle_from_strong(filter: &Strong<dyn IFilter>) -> BinderResult<AidlObjectHandle> {
    let binder_native: Binder<BnFilter> = filter.as_binder().try_into().map_err(|_| {
        service_error(
            TunerResult::INVALID_ARGUMENT.0,
            "source filter is not a local HAL filter",
        )
    })?;
    let Some(local_filter) = binder_native.downcast_binder::<FilterAidlObject>() else {
        return Err(service_error(
            TunerResult::INVALID_ARGUMENT.0,
            "source filter is not a local HAL filter",
        ));
    };
    Ok(local_filter.handle())
}

fn filter_entry_public_id_and_owner(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
) -> BinderResult<(i32, RuntimeOwnerRelation)> {
    let runtime = runtime
        .lock()
        .map_err(|_| status_unknown_error("service runtime lock poisoned"))?;
    let entry = runtime
        .object_table()
        .entry_for_kind(handle.object_id(), handle.generation(), AidlObjectKind::Filter)
        .map_err(|error| match error {
            maleicacid_tuner_hal2_service_runtime::object_table::RuntimeObjectTableError::ObjectKindMismatch { .. }
            | maleicacid_tuner_hal2_service_runtime::object_table::RuntimeObjectTableError::InvalidOwner { .. }
            | maleicacid_tuner_hal2_service_runtime::object_table::RuntimeObjectTableError::OwnerKindMismatch { .. } => {
                service_error(TunerResult::INVALID_ARGUMENT.0, "source filter owner or kind mismatch")
            }
            _ => service_error(TunerResult::INVALID_STATE.0, "source filter is not live"),
        })?;
    let public_id = i32::try_from(entry.ledger_id.0)
        .map_err(|_| status_unknown_error("filter runtime id out of i32 range"))?;
    Ok((public_id, entry.owner))
}

impl IFrontend for FrontendAidlObject {
    fn setCallback(&self, callback: &Strong<dyn IFrontendCallback>) -> BinderResult<()> {
        self.plan_method(AidlMethodCall::FrontendSetCallback)?;
        self.retain_callback(callback)
    }
    fn tune(&self, settings: &FrontendSettings) -> BinderResult<()> {
        self.ensure_open()?;
        let request = aidl_frontend_settings_to_request(settings).map_err(status_from_hal_error)?;
        let runtime = self.runtime();
        let frontend_id =
            runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Frontend)?;
        let entry = runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .validate_frontend_request_for_id(frontend_id, &request)
            .map_err(status_from_hal_error)?;
        self.plan_method(AidlMethodCall::FrontendTune(request.clone()))?;
        start_frontend_backend_tune_worker(
            runtime,
            frontend_id,
            entry,
            request,
            FrontendWorkerKind::Tune,
        )
        .map_err(status_from_hal_error)
    }
    fn stopTune(&self) -> BinderResult<()> {
        self.ensure_open()?;
        self.plan_method(AidlMethodCall::FrontendStopTune)?;
        let runtime = self.runtime();
        let frontend_id =
            runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Frontend)?;
        stop_frontend_tune_worker(
            runtime.clone(),
            frontend_id,
            FrontendWorkerCancelReason::StopRequested,
        )
        .map_err(status_from_hal_error)?;
        stop_frontend_live_data_and_unbind(runtime, frontend_id).map_err(status_from_hal_error)
    }
    fn close(&self) -> BinderResult<()> {
        self.ensure_open()?;
        self.plan_method(AidlMethodCall::FrontendClose)?;
        let runtime = self.runtime();
        let frontend_id =
            runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Frontend)?;
        let closed_lnb_ids = runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .close_lnb_from_frontend_owner_loss(frontend_id)
            .map_err(status_from_hal_error)?;
        for lnb_id in closed_lnb_ids {
            clear_live_lnb_callback_for_public_id(&runtime, lnb_id)?;
        }
        close_frontend_workers_and_live_data(
            runtime,
            frontend_id,
            FrontendWorkerCancelReason::FrontendClosing,
        )
        .map_err(status_from_hal_error)?;
        self.close_object()
    }
    fn scan(&self, settings: &FrontendSettings, scan_type: FrontendScanType) -> BinderResult<()> {
        self.ensure_open()?;
        let scan_mode = aidl_scan_type_to_mode(scan_type).map_err(status_from_hal_error)?;
        let request = aidl_frontend_settings_to_request(settings).map_err(status_from_hal_error)?;
        let runtime = self.runtime();
        let frontend_id =
            runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Frontend)?;
        let entry = runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .validate_frontend_request_for_id(frontend_id, &request)
            .map_err(status_from_hal_error)?;
        let candidates = runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .scan_candidates_for_frontend_entry(&entry, &request, scan_mode)
            .map_err(status_from_hal_error)?;
        self.plan_method(AidlMethodCall::FrontendScan(request.clone()))?;
        start_frontend_backend_scan_session_worker(
            runtime.clone(),
            frontend_id,
            entry,
            request,
            scan_mode,
            candidates,
            scan_end_notifier(runtime, self.handle()),
        )
        .map_err(status_from_hal_error)
    }
    fn stopScan(&self) -> BinderResult<()> {
        self.ensure_open()?;
        self.plan_method(AidlMethodCall::FrontendStopScan)?;
        let runtime = self.runtime();
        let frontend_id =
            runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Frontend)?;
        stop_frontend_scan_worker(
            runtime,
            frontend_id,
            FrontendWorkerCancelReason::StopRequested,
        )
        .map_err(status_from_hal_error)
    }
    fn getStatus(&self, status_types: &[FrontendStatusType]) -> BinderResult<Vec<FrontendStatus>> {
        self.ensure_open()?;
        self.plan_method(unsupported_public_api_call(
            AidlObjectKind::Frontend,
            AidlApi::FrontendGetStatus,
            None,
        ))?;
        let entry = runtime_frontend_entry_for_object(&self.runtime(), self.handle())?;
        let signal_state = frontend_signal_state_for_object(&self.runtime(), self.handle())?;
        frontend_status_for_types(&entry, signal_state, status_types)
    }
    fn setLnb(&self, lnb_id: i32) -> BinderResult<()> {
        self.ensure_open()?;
        let runtime = self.runtime();
        let entry = runtime_frontend_entry_for_object(&runtime, self.handle())?;
        let frontend_id = entry.id.0;
        let exported_lnb_id = {
            let guard = runtime
                .lock()
                .map_err(|_| status_unknown_error("service runtime lock poisoned"))?;
            guard
                .lnb_for_frontend_id(frontend_id)
                .ok_or_else(|| {
                    service_error(TunerResult::UNAVAILABLE.0, "frontend has no exported LNB")
                })?
                .id
                .0
        };
        if exported_lnb_id != lnb_id {
            return Err(service_error(
                TunerResult::INVALID_ARGUMENT.0,
                "LNB does not belong to this frontend",
            ));
        }
        self.plan_method(AidlMethodCall::FrontendSetLnb { lnb_id })?;
        let result = runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .set_frontend_lnb(frontend_id, lnb_id)
            .map_err(status_from_hal_error);
        result
    }
    fn linkCiCam(&self, _ci_cam_id: i32) -> BinderResult<i32> {
        unavailable_after_object_public_api_plan(
            self.plan_method(unsupported_public_api_call(
                AidlObjectKind::Frontend,
                AidlApi::FrontendLinkCiCam,
                None,
            )),
            "CI CAM is unsupported",
        )?;
        Err(status_unknown_error(
            "linkCiCam unavailable path unexpectedly returned success",
        ))
    }
    fn unlinkCiCam(&self, _ci_cam_id: i32) -> BinderResult<()> {
        unavailable_after_object_public_api_plan(
            self.plan_method(unsupported_public_api_call(
                AidlObjectKind::Frontend,
                AidlApi::FrontendUnlinkCiCam,
                None,
            )),
            "CI CAM is unsupported",
        )
    }
    fn getHardwareInfo(&self) -> BinderResult<String> {
        unavailable_after_object_public_api_plan(
            self.plan_method(unsupported_public_api_call(
                AidlObjectKind::Frontend,
                AidlApi::FrontendGetHardwareInfo,
                None,
            )),
            "frontend backend is not probed",
        )?;
        Err(status_unknown_error(
            "getHardwareInfo unavailable path unexpectedly returned success",
        ))
    }
    fn removeOutputPid(&self, _pid: i32) -> BinderResult<()> {
        unavailable_after_object_public_api_plan(
            self.plan_method(unsupported_public_api_call(
                AidlObjectKind::Frontend,
                AidlApi::FrontendRemoveOutputPid,
                None,
            )),
            "frontend output PID removal is unsupported",
        )
    }
    fn getFrontendStatusReadiness(
        &self,
        status_types: &[FrontendStatusType],
    ) -> BinderResult<Vec<FrontendStatusReadiness>> {
        self.ensure_open()?;
        self.plan_method(unsupported_public_api_call(
            AidlObjectKind::Frontend,
            AidlApi::FrontendGetFrontendStatusReadiness,
            None,
        ))?;
        let entry = runtime_frontend_entry_for_object(&self.runtime(), self.handle())?;
        let runtime_state = frontend_runtime_state_for_object(&self.runtime(), self.handle())?;
        let signal_state = frontend_signal_state_for_object(&self.runtime(), self.handle())?;
        Ok(frontend_readiness_for_types(
            &entry,
            runtime_state,
            signal_state,
            status_types,
        ))
    }
}

impl IDemux for DemuxAidlObject {
    fn setFrontendDataSource(&self, frontend_id: i32) -> BinderResult<()> {
        self.ensure_open()?;
        self.plan_method(AidlMethodCall::DemuxSetFrontendDataSource { frontend_id })?;
        let runtime = self.runtime();
        let demux_id = runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Demux)?;
        runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .set_demux_frontend_data_source(demux_id, frontend_id)
            .map_err(status_from_hal_error)?;
        Ok(())
    }
    fn openFilter(
        &self,
        filter_type: &DemuxFilterType,
        buffer_size: i32,
        cb: &Strong<dyn IFilterCallback>,
    ) -> BinderResult<Strong<dyn IFilter>> {
        self.ensure_open()?;
        let open_request = build_open_filter_request(filter_type, buffer_size, true)
            .map_err(status_from_hal_error)?;
        self.plan_method(AidlMethodCall::DemuxOpenFilter(
            maleicacid_tuner_hal2_binder_adapter::RuntimeExecutableRequest::OpenFilter(
                open_request.clone(),
            ),
        ))?;
        let runtime = self.runtime();
        let owner_demux_id =
            runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Demux)?;
        open_filter_child_after_plan(&runtime, self.handle(), owner_demux_id, open_request, cb)
    }
    fn openTimeFilter(&self) -> BinderResult<Strong<dyn ITimeFilter>> {
        unavailable_after_object_public_api_plan(
            self.plan_method(unsupported_public_api_call(
                AidlObjectKind::Demux,
                AidlApi::DemuxOpenTimeFilter,
                None,
            )),
            "time filter is unsupported",
        )?;
        Err(status_unknown_error(
            "openTimeFilter unavailable path unexpectedly returned success",
        ))
    }
    fn getAvSyncHwId(&self, _filter: &Strong<dyn IFilter>) -> BinderResult<i32> {
        unavailable_after_object_public_api_plan(
            self.plan_method(unsupported_public_api_call(
                AidlObjectKind::Demux,
                AidlApi::DemuxGetAvSyncHwId,
                None,
            )),
            "AV sync is not connected in current tuner_hal2 scope",
        )?;
        Err(status_unknown_error(
            "getAvSyncHwId unavailable path unexpectedly returned success",
        ))
    }
    fn getAvSyncTime(&self, _av_sync_hw_id: i32) -> BinderResult<i64> {
        unavailable_after_object_public_api_plan(
            self.plan_method(unsupported_public_api_call(
                AidlObjectKind::Demux,
                AidlApi::DemuxGetAvSyncTime,
                None,
            )),
            "AV sync is not connected in current tuner_hal2 scope",
        )?;
        Err(status_unknown_error(
            "getAvSyncTime unavailable path unexpectedly returned success",
        ))
    }
    fn close(&self) -> BinderResult<()> {
        self.close_object_after_plan(AidlMethodCall::DemuxClose)
    }
    fn openDvr(
        &self,
        dvr_type: DvrType,
        buffer_size: i32,
        cb: &Strong<dyn IDvrCallback>,
    ) -> BinderResult<Strong<dyn IDvr>> {
        self.ensure_open()?;
        let request =
            build_dvr_open_request(dvr_type, buffer_size).map_err(status_from_hal_error)?;
        self.plan_method(AidlMethodCall::DemuxOpenDvr(request))?;
        let runtime = self.runtime();
        let owner_demux_id =
            runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Demux)?;
        open_dvr_child_after_plan(&runtime, self.handle(), owner_demux_id, request, cb)
    }
    fn connectCiCam(&self, _ci_cam_id: i32) -> BinderResult<()> {
        unavailable_after_object_public_api_plan(
            self.plan_method(unsupported_public_api_call(
                AidlObjectKind::Demux,
                AidlApi::DemuxConnectCiCam,
                None,
            )),
            "CI CAM is unsupported",
        )
    }
    fn disconnectCiCam(&self) -> BinderResult<()> {
        unavailable_after_object_public_api_plan(
            self.plan_method(unsupported_public_api_call(
                AidlObjectKind::Demux,
                AidlApi::DemuxDisconnectCiCam,
                None,
            )),
            "CI CAM is unsupported",
        )
    }
}

impl IFilter for FilterAidlObject {
    fn getQueueDesc(&self, _queue: &mut TunerQueueDesc) -> BinderResult<()> {
        self.ensure_open()?;
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::FilterGetQueueDesc),
            "FMQ runtime is not connected in current tuner_hal2 scope",
        )
    }
    fn close(&self) -> BinderResult<()> {
        self.close_object_after_plan(AidlMethodCall::FilterClose)
    }
    fn configure(&self, settings: &DemuxFilterSettings) -> BinderResult<()> {
        self.ensure_open()?;
        let open_type = current_filter_open_type(self)?;
        let config = build_filter_summary_for_open_type(settings, open_type)
            .map_err(status_from_hal_error)?;
        self.plan_method(AidlMethodCall::FilterConfigure(
            maleicacid_tuner_hal2_binder_adapter::RuntimeExecutableRequest::ConfigureFilter(
                config.clone(),
            ),
        ))?;
        let runtime = self.runtime();
        let filter_id = runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Filter)?;
        let result = runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .configure_filter_runtime_request(filter_id, config)
            .map_err(status_from_hal_error);
        result
    }
    fn configureAvStreamType(&self, av_stream_type: &AvStreamType) -> BinderResult<()> {
        self.ensure_open()?;
        let request =
            build_filter_av_stream_type_request(av_stream_type).map_err(status_from_hal_error)?;
        self.plan_method(AidlMethodCall::FilterConfigureAvStreamType(request))?;
        let runtime = self.runtime();
        let filter_id = runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Filter)?;
        let result = runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .configure_filter_av_stream_type_request(filter_id, request)
            .map_err(status_from_hal_error);
        result
    }
    fn configureIpCid(&self, _ip_cid: i32) -> BinderResult<()> {
        self.ensure_open()?;
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::FilterConfigure(maleicacid_tuner_hal2_binder_adapter::RuntimeExecutableRequest::UnsupportedProfile { reason: "IP CID filtering is outside the TS-only tuner_hal2 profile" })),
            "IP CID filtering is outside the TS-only tuner_hal2 profile",
        )
    }
    fn configureMonitorEvent(&self, monitor_event_types: i32) -> BinderResult<()> {
        self.ensure_open()?;
        let plan = self.plan_method(AidlMethodCall::FilterConfigure(
            maleicacid_tuner_hal2_binder_adapter::RuntimeExecutableRequest::UnsupportedProfile {
                reason: "monitor event filtering is outside the TS-only tuner_hal2 profile",
            },
        ))?;
        if monitor_event_types == 0 {
            drop(plan);
            Ok(())
        } else {
            unavailable_after_method_plan(
                Ok(plan),
                "non-zero monitor event mask is outside the TS-only tuner_hal2 profile",
            )
        }
    }
    fn start(&self) -> BinderResult<()> {
        self.ensure_open()?;
        self.plan_method(AidlMethodCall::FilterStart)?;
        let runtime = self.runtime();
        let filter_id = runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Filter)?;
        let result = runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .start_filter_runtime(filter_id)
            .map_err(status_from_hal_error);
        result
    }
    fn stop(&self) -> BinderResult<()> {
        self.ensure_open()?;
        self.plan_method(AidlMethodCall::FilterStop)?;
        let runtime = self.runtime();
        let filter_id = runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Filter)?;
        let result = runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .stop_filter_runtime(filter_id)
            .map_err(status_from_hal_error);
        result
    }
    fn flush(&self) -> BinderResult<()> {
        self.ensure_open()?;
        self.plan_method(AidlMethodCall::FilterFlush)?;
        let runtime = self.runtime();
        let filter_id = runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Filter)?;
        let result = runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .flush_filter_runtime(filter_id)
            .map_err(status_from_hal_error);
        result
    }
    fn getAvSharedHandle(&self, _av_memory: &mut TunerNativeHandle) -> BinderResult<i64> {
        self.ensure_open()?;
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::FilterGetAvSharedHandle),
            "AV shared memory is not connected in current tuner_hal2 scope",
        )?;
        Err(status_unknown_error(
            "getAvSharedHandle unavailable path unexpectedly returned success",
        ))
    }
    fn getId(&self) -> BinderResult<i32> {
        self.ensure_open()?;
        self.plan_method(AidlMethodCall::FilterGetId)?;
        let runtime = self.runtime();
        runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Filter)
    }
    fn getId64Bit(&self) -> BinderResult<i64> {
        self.ensure_open()?;
        self.plan_method(AidlMethodCall::FilterGetId64Bit)?;
        let runtime = self.runtime();
        let filter_id = runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Filter)?;
        Ok(i64::from(filter_id))
    }
    fn releaseAvHandle(&self, _av_memory: &TunerNativeHandle, av_data_id: i64) -> BinderResult<()> {
        self.ensure_open()?;
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::FilterReleaseAvHandle(
                FilterReleaseAvHandleRequest { av_data_id },
            )),
            "AV shared memory is not connected in current tuner_hal2 scope",
        )
    }
    fn setDataSource(&self, filter: &Strong<dyn IFilter>) -> BinderResult<()> {
        self.ensure_open()?;
        let runtime = self.runtime();
        let sink_handle = self.handle();
        let source_handle = local_filter_handle_from_strong(filter)?;
        if source_handle.object_id() == sink_handle.object_id()
            && source_handle.generation() == sink_handle.generation()
        {
            return Err(service_error(
                TunerResult::INVALID_ARGUMENT.0,
                "filter cannot use itself as source",
            ));
        }
        let (sink_id, sink_owner) = filter_entry_public_id_and_owner(&runtime, sink_handle)?;
        let (source_id, source_owner) = filter_entry_public_id_and_owner(&runtime, source_handle)?;
        if sink_owner != source_owner {
            return Err(service_error(
                TunerResult::INVALID_ARGUMENT.0,
                "source filter belongs to a different demux",
            ));
        }
        let RuntimeOwnerRelation::Demux { demux, generation } = sink_owner else {
            return Err(service_error(
                TunerResult::INVALID_ARGUMENT.0,
                "filter owner is not a demux",
            ));
        };
        let demux_id = {
            let runtime_guard = runtime
                .lock()
                .map_err(|_| status_unknown_error("service runtime lock poisoned"))?;
            let demux_entry = runtime_guard
                .object_table()
                .entry_for_kind(demux, generation, AidlObjectKind::Demux)
                .map_err(|_| {
                    service_error(TunerResult::INVALID_STATE.0, "owner demux is not live")
                })?;
            i32::try_from(demux_entry.ledger_id.0)
                .map_err(|_| status_unknown_error("demux runtime id out of i32 range"))?
        };
        self.plan_method(AidlMethodCall::FilterSetDataSource(
            FilterSetDataSourceRequest {
                source_filter_id: source_handle.object_id().0,
                source_filter_generation: source_handle.generation().0,
            },
        ))?;
        runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .set_filter_data_source_non_null(demux_id, sink_id, source_id)
            .map_err(status_from_hal_error)?;
        Ok(())
    }
    fn setDelayHint(&self, hint: &FilterDelayHint) -> BinderResult<()> {
        self.ensure_open()?;
        let request = build_filter_delay_hint_request(hint).map_err(status_from_hal_error)?;
        self.plan_method(AidlMethodCall::FilterSetDelayHint(request))?;
        let runtime = self.runtime();
        let filter_id = runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Filter)?;
        let result = runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .set_filter_delay_hint_request(filter_id, request)
            .map_err(status_from_hal_error);
        result
    }
}

impl IDvr for DvrAidlObject {
    fn getQueueDesc(&self, _queue: &mut TunerQueueDesc) -> BinderResult<()> {
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::DvrGetQueueDesc),
            "DVR FMQ runtime is not connected in current tuner_hal2 scope",
        )
    }
    fn configure(&self, settings: &DvrSettings) -> BinderResult<()> {
        let request = build_dvr_configure_request(settings).map_err(status_from_hal_error)?;
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::DvrConfigure(request)),
            "DVR runtime is not connected in current tuner_hal2 scope",
        )
    }
    fn attachFilter(&self, filter: &Strong<dyn IFilter>) -> BinderResult<()> {
        self.ensure_open()?;
        let filter_handle = local_filter_handle_from_strong(filter)?;
        let request = DvrFilterLinkRequest {
            filter_id: filter_handle.object_id().0,
            filter_generation: filter_handle.generation().0,
        };
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::DvrAttachFilter(request)),
            "DVR runtime is not connected in current tuner_hal2 scope",
        )
    }
    fn detachFilter(&self, filter: &Strong<dyn IFilter>) -> BinderResult<()> {
        self.ensure_open()?;
        let filter_handle = local_filter_handle_from_strong(filter)?;
        let request = DvrFilterLinkRequest {
            filter_id: filter_handle.object_id().0,
            filter_generation: filter_handle.generation().0,
        };
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::DvrDetachFilter(request)),
            "DVR runtime is not connected in current tuner_hal2 scope",
        )
    }
    fn start(&self) -> BinderResult<()> {
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::DvrStart),
            "DVR runtime is not connected in current tuner_hal2 scope",
        )
    }
    fn stop(&self) -> BinderResult<()> {
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::DvrStop),
            "DVR runtime is not connected in current tuner_hal2 scope",
        )
    }
    fn flush(&self) -> BinderResult<()> {
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::DvrFlush),
            "DVR runtime is not connected in current tuner_hal2 scope",
        )
    }
    fn close(&self) -> BinderResult<()> {
        self.close_object_after_plan(AidlMethodCall::DvrClose)
    }
    fn setStatusCheckIntervalHint(&self, milliseconds: i64) -> BinderResult<()> {
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::DvrSetStatusCheckIntervalHint(milliseconds)),
            "DVR callback runtime is not connected in current tuner_hal2 scope",
        )
    }
}

impl IDescrambler for DescramblerAidlObject {
    fn setDemuxSource(&self, demux_id: i32) -> BinderResult<()> {
        self.ensure_open()?;
        self.plan_method(AidlMethodCall::DescramblerSetDemuxSource(demux_id))?;
        let runtime = self.runtime();
        let descrambler_id =
            runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Descrambler)?;
        let result = runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .set_descrambler_demux_source(descrambler_id, demux_id)
            .map_err(status_from_hal_error);
        result
    }
    fn setKeyToken(&self, key_token: &[u8]) -> BinderResult<()> {
        self.ensure_open()?;
        self.plan_method(AidlMethodCall::DescramblerSetKeyToken(key_token.to_vec()))?;
        let runtime = self.runtime();
        let descrambler_id =
            runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Descrambler)?;
        let result = runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .set_descrambler_key_token(descrambler_id, key_token)
            .map_err(status_from_hal_error);
        result
    }
    fn addPid(
        &self,
        pid: &DemuxPid,
        optional_upstream_filter: &Strong<dyn IFilter>,
    ) -> BinderResult<()> {
        self.ensure_open()?;
        let pid = ts_pid_from_demux_pid(pid).map_err(status_from_hal_error)?;
        let source_handle = local_filter_handle_from_strong(optional_upstream_filter)?;
        let self_runtime = self.runtime();
        let descrambler_id =
            runtime_entry_public_id(&self_runtime, self.handle(), AidlObjectKind::Descrambler)?;
        let (source_filter_id, _) = filter_entry_public_id_and_owner(&self_runtime, source_handle)?;
        self.plan_method(AidlMethodCall::DescramblerAddPid(pid))?;
        let result = self_runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .add_descrambler_pid_non_null_source(descrambler_id, pid, source_filter_id)
            .map_err(status_from_hal_error);
        result
    }
    fn removePid(
        &self,
        pid: &DemuxPid,
        optional_upstream_filter: &Strong<dyn IFilter>,
    ) -> BinderResult<()> {
        self.ensure_open()?;
        let pid = ts_pid_from_demux_pid(pid).map_err(status_from_hal_error)?;
        let source_handle = local_filter_handle_from_strong(optional_upstream_filter)?;
        let self_runtime = self.runtime();
        let descrambler_id =
            runtime_entry_public_id(&self_runtime, self.handle(), AidlObjectKind::Descrambler)?;
        let (source_filter_id, _) = filter_entry_public_id_and_owner(&self_runtime, source_handle)?;
        self.plan_method(AidlMethodCall::DescramblerRemovePid(pid))?;
        let result = self_runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .remove_descrambler_pid_non_null_source(descrambler_id, pid, source_filter_id)
            .map_err(status_from_hal_error);
        result
    }
    fn close(&self) -> BinderResult<()> {
        self.close_object_after_plan(AidlMethodCall::DescramblerClose)
    }
}

impl ILnb for LnbAidlObject {
    fn setCallback(&self, callback: &Strong<dyn ILnbCallback>) -> BinderResult<()> {
        self.ensure_open()?;
        self.plan_method(AidlMethodCall::LnbSetCallback)?;
        self.retain_callback(callback)?;
        let runtime = self.runtime();
        let lnb_id = match runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Lnb) {
            Ok(id) => id,
            Err(status) => {
                self.rollback_callback_registration()?;
                return Err(status);
            }
        };
        let result = match runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .mark_lnb_callback_registered(lnb_id)
        {
            Ok(()) => Ok(()),
            Err(error) => {
                self.rollback_callback_registration()?;
                Err(status_from_hal_error(error))
            }
        };
        result
    }
    fn setVoltage(&self, voltage: LnbVoltage) -> BinderResult<()> {
        self.ensure_open()?;
        let request = build_lnb_voltage_request(voltage).map_err(status_from_hal_error)?;
        self.plan_method(AidlMethodCall::LnbSetVoltage(request))?;
        let runtime = self.runtime();
        let lnb_id = runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Lnb)?;
        let result = runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .apply_lnb_voltage(lnb_id, request)
            .map_err(status_from_hal_error);
        result
    }
    fn setTone(&self, tone: LnbTone) -> BinderResult<()> {
        self.ensure_open()?;
        let request = build_lnb_tone_request(tone).map_err(status_from_hal_error)?;
        self.plan_method(AidlMethodCall::LnbSetTone(request))?;
        let runtime = self.runtime();
        let lnb_id = runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Lnb)?;
        let result = runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .apply_lnb_tone(lnb_id, request)
            .map_err(status_from_hal_error);
        result
    }
    fn setSatellitePosition(&self, position: LnbPosition) -> BinderResult<()> {
        self.ensure_open()?;
        let request =
            build_lnb_satellite_position_request(position).map_err(status_from_hal_error)?;
        self.plan_method(AidlMethodCall::LnbSetSatellitePosition(request))?;
        let runtime = self.runtime();
        let lnb_id = runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Lnb)?;
        let result = runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .apply_lnb_satellite_position(lnb_id, request)
            .map_err(status_from_hal_error);
        result
    }
    fn sendDiseqcMessage(&self, diseqc_message: &[u8]) -> BinderResult<()> {
        self.ensure_open()?;
        self.plan_method(AidlMethodCall::LnbSendDiseqc(diseqc_message.to_vec()))?;
        let runtime = self.runtime();
        let lnb_id = runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Lnb)?;
        let result = runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .send_lnb_diseqc(lnb_id, diseqc_message)
            .map_err(status_from_hal_error);
        result
    }
    fn close(&self) -> BinderResult<()> {
        self.ensure_open()?;
        self.plan_method(AidlMethodCall::LnbClose)?;
        let runtime = self.runtime();
        let lnb_id = runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Lnb)?;
        runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .close_lnb_explicit(lnb_id)
            .map_err(status_from_hal_error)?;
        self.close_object()
    }
}

fn status_unknown_error(message: &str) -> Status {
    service_error(TunerResult::UNKNOWN_ERROR.0, message)
}
fn status_from_hal_error(error: HalError) -> Status {
    let status = AidlStatusMapper::map_error(&error);
    status_from_tuner_status(status, &error.to_string())
}
fn status_from_tuner_status(status: TunerStatusCode, message: &str) -> Status {
    match status {
        TunerStatusCode::Ok => service_error(
            TunerResult::UNKNOWN_ERROR.0,
            "unexpected OK status while building error",
        ),
        TunerStatusCode::InvalidArgument => service_error(TunerResult::INVALID_ARGUMENT.0, message),
        TunerStatusCode::InvalidState => service_error(TunerResult::INVALID_STATE.0, message),
        TunerStatusCode::Unavailable => service_error(TunerResult::UNAVAILABLE.0, message),
        TunerStatusCode::UnknownError => service_error(TunerResult::UNKNOWN_ERROR.0, message),
    }
}
fn service_error(code: i32, message: &str) -> Status {
    match CString::new(message) {
        Ok(detail) => Status::new_service_specific_error(code, Some(detail.as_c_str())),
        Err(_) => Status::new_service_specific_error(code, None),
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
                    tx.send((ctx.kind(), ctx.cancel_reason())).unwrap();
                    Ok(())
                })
                .unwrap();
            runtime
                .start_frontend_worker(7, FrontendWorkerKind::Scan, 1, move |ctx| {
                    while !ctx.cancel_requested() {
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                    reason_tx.send((ctx.kind(), ctx.cancel_reason())).unwrap();
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
