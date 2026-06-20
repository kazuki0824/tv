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
    IFilter::IFilter,
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
use binder::{BinderFeatures, Interface, Result as BinderResult, Status, Strong};
use maleicacid_tuner_hal2_binder_adapter::{
    aidl_frontend_settings_to_request, aidl_scan_type_to_mode, build_dvr_configure_request,
    build_dvr_open_request, build_filter_av_stream_type_request, build_filter_delay_hint_request,
    build_filter_summary_for_open_type, build_lnb_satellite_position_request,
    build_lnb_tone_request, build_lnb_voltage_request, build_open_filter_request, AidlApi,
    AidlMethodAdapter, AidlMethodCall, AidlMethodPlan, AidlObjectGeneration, AidlObjectId,
    AidlObjectKind, DvrFilterLinkRequest, FilterReleaseAvHandleRequest, FilterSetDataSourceRequest,
};
use maleicacid_tuner_hal2_common::{
    fail_after_cleanup, japan_isdbt_frequency_contract_range_hz, FrontendBackendKind,
    FrontendSystem, HalError, HalInternalKind,
};
use maleicacid_tuner_hal2_device::{FrontendRuntimeState, FrontendSignalState};
use maleicacid_tuner_hal2_service_runtime::{
    close_frontend_object_cleanup_use_case, set_frontend_lnb_object_use_case,
    start_frontend_scan_use_case, start_frontend_tune_use_case, stop_frontend_scan_use_case,
    stop_frontend_tune_use_case, FrontendRegistryEntry, FrontendRuntimeId, LnbRegistryProfile,
    RuntimeCommandDispatchError, RuntimeCommandDispatchPlan, RuntimeObjectEntry,
    TunerServiceRuntime,
};

use crate::child_object_open::{
    open_dvr_child_for_owner_object_with_request_builder,
    open_filter_child_for_owner_object_with_request_builder,
};
use crate::demux_object::DemuxAidlObject;
use crate::descrambler_object::DescramblerAidlObject;
use crate::dvr_object::DvrAidlObject;
use crate::error_bridge::{service_error, status_from_hal_error, status_unknown_error};
use crate::filter_object::FilterAidlObject;
use crate::frontend_callback_delivery::scan_end_notifier;
use crate::frontend_object::FrontendAidlObject;
use crate::lnb_object::LnbAidlObject;
use crate::object_handle::AidlObjectHandle;
use crate::object_runtime::{
    close_object_after_close_preflight, close_object_after_close_preflight_with_domain_cleanup,
    execute_object_query_use_case, execute_object_runtime_use_case,
    execute_object_runtime_use_case_with_request_builder, execute_shared_object_runtime_use_case,
    execute_shared_object_runtime_use_case_with_request_builder,
    plan_unavailable_object_method_use_case,
};

mod demux_methods;
mod descrambler_methods;
mod dvr_methods;
mod filter_methods;
mod frontend_methods;
mod lnb_methods;
mod support;

use self::support::{
    plan_tuner_public_api_method, public_api_call, unavailable_after_tuner_method_plan,
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

fn finish_hal_cleanup_after_primary<T>(
    context: &'static str,
    primary: HalError,
    cleanup: Result<(), HalError>,
) -> BinderResult<T> {
    fail_after_cleanup(context, primary, cleanup).map_err(status_from_hal_error)
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

    pub fn plan_from_method_plan(
        &self,
        method_plan: &AidlMethodPlan,
    ) -> Result<RuntimeCommandDispatchPlan, RuntimeCommandDispatchError> {
        match self.runtime.lock() {
            Ok(mut runtime) => runtime.plan_command_dispatch(
                method_plan.command_plan,
                method_plan.command.runtime_executable_request(),
            ),
            Err(_) => Err(RuntimeCommandDispatchError::RuntimeLockPoison {
                transaction: method_plan.command_plan.transaction(),
            }),
        }
    }

    fn handle_from_runtime_entry(entry: RuntimeObjectEntry) -> AidlObjectHandle {
        AidlObjectHandle::new(entry.object_kind, entry.object_id, entry.generation)
    }

    fn rollback_root_object_entry_after_aidl_failure_hal(
        &self,
        entry: RuntimeObjectEntry,
        unregister_runtime: bool,
    ) -> Result<(), HalError> {
        self.runtime
            .lock()
            .map_err(|_| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "service runtime lock poisoned",
                )
            })?
            .rollback_root_object_entry_after_aidl_failure(entry, unregister_runtime)
    }

    fn frontend_object_from_entry(
        &self,
        entry: RuntimeObjectEntry,
    ) -> BinderResult<Strong<dyn IFrontend>> {
        if i32::try_from(entry.ledger_id.0).is_err() {
            return finish_hal_cleanup_after_primary(
                "frontend root object runtime id conversion rollback failed",
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime id is outside i32 range",
                ),
                self.rollback_root_object_entry_after_aidl_failure_hal(entry.clone(), false),
            );
        }
        let handle = Self::handle_from_runtime_entry(entry.clone());
        match FrontendAidlObject::new(handle, self.runtime.clone()) {
            Ok(object) => Ok(BnFrontend::new_binder(object, BinderFeatures::default())),
            Err(_) => finish_hal_cleanup_after_primary(
                "frontend root object construction rollback failed",
                HalError::internal(HalInternalKind::InvariantViolation, "object kind mismatch"),
                self.rollback_root_object_entry_after_aidl_failure_hal(entry, false),
            ),
        }
    }

    fn demux_object_from_entry(
        &self,
        entry: RuntimeObjectEntry,
        unregister_runtime_on_failure: bool,
    ) -> BinderResult<(Strong<dyn IDemux>, i32)> {
        let public_id = match i32::try_from(entry.ledger_id.0) {
            Ok(public_id) => public_id,
            Err(_) => {
                return finish_hal_cleanup_after_primary(
                    "demux root object runtime id conversion rollback failed",
                    HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "demux runtime id is outside i32 range",
                    ),
                    self.rollback_root_object_entry_after_aidl_failure_hal(
                        entry.clone(),
                        unregister_runtime_on_failure,
                    ),
                );
            }
        };
        let handle = Self::handle_from_runtime_entry(entry.clone());
        match DemuxAidlObject::new(handle, self.runtime.clone()) {
            Ok(object) => Ok((
                BnDemux::new_binder(object, BinderFeatures::default()),
                public_id,
            )),
            Err(_) => finish_hal_cleanup_after_primary(
                "demux root object construction rollback failed",
                HalError::internal(HalInternalKind::InvariantViolation, "object kind mismatch"),
                self.rollback_root_object_entry_after_aidl_failure_hal(
                    entry,
                    unregister_runtime_on_failure,
                ),
            ),
        }
    }

    fn descrambler_object_from_entry(
        &self,
        entry: RuntimeObjectEntry,
    ) -> BinderResult<Strong<dyn IDescrambler>> {
        if i32::try_from(entry.ledger_id.0).is_err() {
            return finish_hal_cleanup_after_primary(
                "descrambler root object runtime id conversion rollback failed",
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "descrambler runtime id is outside i32 range",
                ),
                self.rollback_root_object_entry_after_aidl_failure_hal(entry.clone(), true),
            );
        }
        let handle = Self::handle_from_runtime_entry(entry.clone());
        match DescramblerAidlObject::new(handle, self.runtime.clone()) {
            Ok(object) => Ok(BnDescrambler::new_binder(object, BinderFeatures::default())),
            Err(_) => finish_hal_cleanup_after_primary(
                "descrambler root object construction rollback failed",
                HalError::internal(HalInternalKind::InvariantViolation, "object kind mismatch"),
                self.rollback_root_object_entry_after_aidl_failure_hal(entry, true),
            ),
        }
    }

    fn lnb_object_from_entry(&self, entry: RuntimeObjectEntry) -> BinderResult<Strong<dyn ILnb>> {
        if i32::try_from(entry.ledger_id.0).is_err() {
            return finish_hal_cleanup_after_primary(
                "LNB root object runtime id conversion rollback failed",
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "LNB runtime id is outside i32 range",
                ),
                self.rollback_root_object_entry_after_aidl_failure_hal(entry.clone(), false),
            );
        }
        let handle = Self::handle_from_runtime_entry(entry.clone());
        match LnbAidlObject::new(handle, self.runtime.clone()) {
            Ok(object) => Ok(BnLnb::new_binder(object, BinderFeatures::default())),
            Err(_) => finish_hal_cleanup_after_primary(
                "LNB root object construction rollback failed",
                HalError::internal(HalInternalKind::InvariantViolation, "object kind mismatch"),
                self.rollback_root_object_entry_after_aidl_failure_hal(entry, false),
            ),
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

impl ITuner for TunerAidlService {
    fn getFrontendIds(&self) -> BinderResult<Vec<i32>> {
        plan_tuner_public_api_method(self, AidlApi::TunerGetFrontendIds, None)?;
        let runtime = self.lock_runtime()?;
        Ok(runtime.frontend_ids())
    }

    fn openFrontendById(&self, frontend_id: i32) -> BinderResult<Strong<dyn IFrontend>> {
        let method_plan = AidlMethodAdapter::plan(public_api_call(
            AidlObjectKind::Tuner,
            AidlApi::TunerOpenFrontendById,
            None,
        ))
        .map_err(status_from_hal_error)?;
        let entry = self
            .lock_runtime()?
            .open_frontend_root_object_for_id(
                frontend_id,
                method_plan.command_plan,
                method_plan.command.runtime_executable_request(),
            )
            .map_err(status_from_hal_error)?;
        self.frontend_object_from_entry(entry)
    }

    fn openDemux(&self, demux_id: &mut Vec<i32>) -> BinderResult<Strong<dyn IDemux>> {
        demux_id.clear();
        let method_plan = AidlMethodAdapter::plan(public_api_call(
            AidlObjectKind::Tuner,
            AidlApi::TunerOpenDemux,
            None,
        ))
        .map_err(status_from_hal_error)?;
        let entry = self
            .lock_runtime()?
            .open_demux_root_object(
                method_plan.command_plan,
                method_plan.command.runtime_executable_request(),
            )
            .map_err(status_from_hal_error)?;
        let (object, id) = self.demux_object_from_entry(entry, true)?;
        demux_id.push(id);
        Ok(object)
    }

    fn getDemuxCaps(&self) -> BinderResult<DemuxCapabilities> {
        plan_tuner_public_api_method(self, AidlApi::TunerGetDemuxCaps, None)?;
        Ok(tuner_hal2_demux_capabilities())
    }

    fn openDescrambler(&self) -> BinderResult<Strong<dyn IDescrambler>> {
        let method_plan = AidlMethodAdapter::plan(public_api_call(
            AidlObjectKind::Tuner,
            AidlApi::TunerOpenDescrambler,
            None,
        ))
        .map_err(status_from_hal_error)?;
        let entry = self
            .lock_runtime()?
            .open_descrambler_root_object(
                method_plan.command_plan,
                method_plan.command.runtime_executable_request(),
            )
            .map_err(status_from_hal_error)?;
        self.descrambler_object_from_entry(entry)
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
        let method_plan = AidlMethodAdapter::plan(public_api_call(
            AidlObjectKind::Tuner,
            AidlApi::TunerOpenLnbById,
            None,
        ))
        .map_err(status_from_hal_error)?;
        let entry = self
            .lock_runtime()?
            .open_lnb_root_object_for_id(
                lnb_id,
                method_plan.command_plan,
                method_plan.command.runtime_executable_request(),
            )
            .map_err(status_from_hal_error)?;
        self.lnb_object_from_entry(entry)
    }

    fn openLnbByName(
        &self,
        lnb_name: &str,
        lnb_id: &mut Vec<i32>,
    ) -> BinderResult<Strong<dyn ILnb>> {
        lnb_id.clear();
        let method_plan = AidlMethodAdapter::plan(public_api_call(
            AidlObjectKind::Tuner,
            AidlApi::TunerOpenLnbByName,
            None,
        ))
        .map_err(status_from_hal_error)?;
        let (id, entry) = self
            .lock_runtime()?
            .open_lnb_root_object_by_name(
                lnb_name,
                method_plan.command_plan,
                method_plan.command.runtime_executable_request(),
            )
            .map_err(status_from_hal_error)?;
        let object = self.lnb_object_from_entry(entry)?;
        lnb_id.push(id);
        Ok(object)
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
        let method_plan = AidlMethodAdapter::plan(public_api_call(
            AidlObjectKind::Tuner,
            AidlApi::TunerOpenDemuxById,
            None,
        ))
        .map_err(status_from_hal_error)?;
        let entry = self
            .lock_runtime()?
            .open_demux_root_object_by_id(
                demux_id,
                method_plan.command_plan,
                method_plan.command.runtime_executable_request(),
            )
            .map_err(status_from_hal_error)?;
        self.demux_object_from_entry(entry, false)
            .map(|(object, _id)| object)
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
    use maleicacid_tuner_hal2_common::FrontendSystem;
    use maleicacid_tuner_hal2_service_runtime::RuntimeOwnerRelation;

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
