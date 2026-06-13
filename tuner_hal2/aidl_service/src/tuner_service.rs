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
    DemuxFilterSubType::DemuxFilterSubType,
    DemuxFilterType::DemuxFilterType,
    DemuxInfo::DemuxInfo,
    DemuxPid::DemuxPid,
    DemuxTsFilterType::DemuxTsFilterType,
    DvrSettings::DvrSettings,
    DvrType::DvrType,
    FilterDelayHint::FilterDelayHint,
    FrontendCapabilities::FrontendCapabilities,
    FrontendInfo::FrontendInfo,
    FrontendIsdbsCapabilities::FrontendIsdbsCapabilities,
    FrontendIsdbsCoderate::FrontendIsdbsCoderate,
    FrontendIsdbsModulation::FrontendIsdbsModulation,
    FrontendIsdbsStreamIdType::FrontendIsdbsStreamIdType,
    FrontendIsdbtBandwidth::FrontendIsdbtBandwidth,
    FrontendIsdbtCapabilities::FrontendIsdbtCapabilities,
    FrontendIsdbtCoderate::FrontendIsdbtCoderate,
    FrontendIsdbtGuardInterval::FrontendIsdbtGuardInterval,
    FrontendIsdbtMode::FrontendIsdbtMode,
    FrontendIsdbtModulation::FrontendIsdbtModulation,
    FrontendIsdbtTimeInterleaveMode::FrontendIsdbtTimeInterleaveMode,
    FrontendScanMessage::FrontendScanMessage,
    FrontendScanMessageType::FrontendScanMessageType,
    FrontendScanType::FrontendScanType,
    FrontendSettings::FrontendSettings,
    FrontendStatus::FrontendStatus,
    FrontendStatusReadiness::FrontendStatusReadiness,
    FrontendStatusType::FrontendStatusType,
    FrontendType::FrontendType,
    IDemux::{BnDemux, IDemux},
    IDescrambler::{BnDescrambler, IDescrambler},
    IDvr::{BnDvr, IDvr},
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
    AidlApi, AidlFailureSource, AidlInputField, AidlInputSnapshot, AidlMethodAdapter,
    AidlMethodCall, AidlMethodPlan, AidlObjectGeneration, AidlObjectId, AidlObjectKind,
    AidlStatusMapper, TunerStatusCode,
};
use maleicacid_tuner_hal2_common::{
    is_japan_bs_if_frequency_hz, is_japan_cs110_if_frequency_hz,
    is_japan_isdbt_frequency_contract_hz, japan_isdbt_frequency_contract_range_hz,
    FrontendBackendKind, FrontendDevicePath, FrontendScanMode, FrontendStreamIdKind,
    FrontendSystem, FrontendTuneRequest, HalError, HalInternalKind, HalInvalidArgumentKind,
};
use maleicacid_tuner_hal2_demux::packet_pipeline::PipelineOpenKind;
use maleicacid_tuner_hal2_device::{
    FrontendBackendSession, FrontendBackendTunePlan, FrontendLivePumpJoinOutcome,
    FrontendRuntimeState, FrontendSignalState, FrontendWorkerCancelReason, FrontendWorkerContext,
    FrontendWorkerKind, FrontendWorkerStartError, FrontendWorkerStopOutcome,
};
use maleicacid_tuner_hal2_service_runtime::{
    start_frontend_demux_live_pump_from_reader, FrontendRegistryEntry, FrontendRuntimeId,
    LnbRegistryProfile, RuntimeCommandDispatchError, RuntimeCommandDispatchPlan,
    RuntimeOwnerRelation, TunerServiceRuntime,
};

use crate::callback_store::frontend_callback_for_owner;
use crate::demux_object::DemuxAidlObject;
use crate::descrambler_object::DescramblerAidlObject;
use crate::dvr_object::DvrAidlObject;
use crate::filter_object::FilterAidlObject;
use crate::frontend_object::FrontendAidlObject;
use crate::input_snapshot::{
    snapshot_av_stream_type, snapshot_demux_open_dvr, snapshot_demux_open_filter,
    snapshot_dvr_settings, snapshot_filter_delay_hint, snapshot_filter_settings,
    snapshot_strong_handle,
};
use crate::lnb_object::LnbAidlObject;
use crate::object_handle::AidlObjectHandle;
use crate::object_runtime::SharedTunerRuntime;

type TunerQueueDesc = CommonMqDescriptor<i8, CommonSynchronizedReadWrite>;
type TunerNativeHandle = CommonNativeHandle;

#[derive(Clone)]
pub struct TunerAidlService {
    runtime: Arc<Mutex<TunerServiceRuntime>>,
}

impl Interface for TunerAidlService {}

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
        plan_tuner_query_method(
            self,
            AidlApi::TunerOpenFrontendById,
            Some(snapshot_value("frontend_id", frontend_id.to_string())),
        )?;
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
        unavailable_after_tuner_method_plan(
            self,
            AidlApi::TunerGetDemuxCaps,
            None,
            "demux capability is not implemented in WP-R03",
        )?;
        Err(status_unknown_error(
            "getDemuxCaps unavailable path unexpectedly returned success",
        ))
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
        plan_tuner_query_method(
            self,
            AidlApi::TunerGetFrontendInfo,
            Some(snapshot_value("frontend_id", frontend_id.to_string())),
        )?;
        let entry = self.frontend_entry(frontend_id)?;
        Ok(frontend_info_from_entry(&entry))
    }

    fn getLnbIds(&self) -> BinderResult<Vec<i32>> {
        plan_tuner_query_method(self, AidlApi::TunerGetLnbIds, None)?;
        let runtime = self.lock_runtime()?;
        Ok(runtime.lnb_ids())
    }

    fn openLnbById(&self, lnb_id: i32) -> BinderResult<Strong<dyn ILnb>> {
        plan_tuner_query_method(
            self,
            AidlApi::TunerOpenLnbById,
            Some(snapshot_value("lnb_id", lnb_id.to_string())),
        )?;
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
        plan_tuner_query_method(
            self,
            AidlApi::TunerOpenLnbByName,
            Some(snapshot_value("lnb_name", lnb_name)),
        )?;
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

    fn setLna(&self, b_enable: bool) -> BinderResult<()> {
        unavailable_after_tuner_method_plan(
            self,
            AidlApi::TunerSetLna,
            Some(snapshot_value("enable", b_enable.to_string())),
            "LNA is unsupported",
        )
    }

    fn setMaxNumberOfFrontends(
        &self,
        frontend_type: FrontendType,
        max_number: i32,
    ) -> BinderResult<()> {
        let input = Some(snapshot_fields(
            "SetMaxNumberOfFrontends",
            vec![
                AidlInputField::new("frontend_type", format!("{:?}", frontend_type)),
                AidlInputField::new("max_number", max_number.to_string()),
            ],
        ));
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

    fn getMaxNumberOfFrontends(&self, frontend_type: FrontendType) -> BinderResult<i32> {
        plan_tuner_query_method(
            self,
            AidlApi::TunerGetMaxNumberOfFrontends,
            Some(snapshot_fields(
                "GetMaxNumberOfFrontends",
                vec![AidlInputField::new(
                    "frontend_type",
                    format!("{:?}", frontend_type),
                )],
            )),
        )?;
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
        plan_tuner_query_method(
            self,
            AidlApi::TunerOpenDemuxById,
            Some(snapshot_value("demux_id", demux_id.to_string())),
        )?;
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
        unavailable_after_tuner_method_plan(
            self,
            AidlApi::TunerGetDemuxInfo,
            Some(snapshot_value("demux_id", demux_id.to_string())),
            "demux info is not available",
        )?;
        Err(status_unknown_error(
            "getDemuxInfo unavailable path unexpectedly returned success",
        ))
    }
}

const AOSP_TUNER_INVALID_STREAM_ID: i32 = -1;

fn cast_u64_field(value: i64, field: &'static str) -> Result<u64, HalError> {
    u64::try_from(value).map_err(|_| {
        HalError::invalid_argument(
            HalInvalidArgumentKind::NumericRange,
            format!("{field} must be non-negative"),
        )
    })
}

fn optional_positive_i64_to_u64_field(
    value: i64,
    field: &'static str,
) -> Result<Option<u64>, HalError> {
    if value < 0 {
        return Err(HalError::invalid_argument(
            HalInvalidArgumentKind::NumericRange,
            format!("{field} must be non-negative"),
        ));
    }
    Ok(u64::try_from(value).ok().filter(|v| *v > 0))
}

fn map_isdbt_bandwidth(bandwidth: FrontendIsdbtBandwidth) -> Option<u32> {
    match bandwidth {
        FrontendIsdbtBandwidth::BANDWIDTH_6MHZ => Some(6_000_000),
        FrontendIsdbtBandwidth::BANDWIDTH_7MHZ => Some(7_000_000),
        FrontendIsdbtBandwidth::BANDWIDTH_8MHZ => Some(8_000_000),
        _ => None,
    }
}

fn validate_isdbt_fixed_settings(
    s: &android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::FrontendIsdbtSettings::FrontendIsdbtSettings,
) -> Result<(), HalError> {
    if !matches!(
        s.bandwidth,
        FrontendIsdbtBandwidth::AUTO | FrontendIsdbtBandwidth::BANDWIDTH_6MHZ
    ) {
        return Err(HalError::invalid_argument(
            HalInvalidArgumentKind::UnsupportedBandwidth,
            "ISDB-T bandwidth must be AUTO or 6MHz",
        ));
    }
    if !matches!(s.mode, FrontendIsdbtMode::AUTO | FrontendIsdbtMode::MODE_3) {
        return Err(HalError::invalid_argument(
            HalInvalidArgumentKind::UnsupportedBandwidth,
            "ISDB-T mode must be AUTO or MODE_3",
        ));
    }
    if !matches!(
        s.guardInterval,
        FrontendIsdbtGuardInterval::AUTO
            | FrontendIsdbtGuardInterval::INTERVAL_1_32
            | FrontendIsdbtGuardInterval::INTERVAL_1_16
            | FrontendIsdbtGuardInterval::INTERVAL_1_8
            | FrontendIsdbtGuardInterval::INTERVAL_1_4
    ) {
        return Err(HalError::invalid_argument(
            HalInvalidArgumentKind::UnsupportedBandwidth,
            "unsupported ISDB-T guard interval",
        ));
    }
    for layer in &s.layerSettings {
        if !matches!(
            layer.modulation,
            FrontendIsdbtModulation::AUTO
                | FrontendIsdbtModulation::MOD_DQPSK
                | FrontendIsdbtModulation::MOD_QPSK
                | FrontendIsdbtModulation::MOD_16QAM
                | FrontendIsdbtModulation::MOD_64QAM
        ) {
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::UnsupportedBandwidth,
                "unsupported ISDB-T layer modulation",
            ));
        }
        if !matches!(
            layer.coderate,
            FrontendIsdbtCoderate::AUTO
                | FrontendIsdbtCoderate::CODERATE_1_2
                | FrontendIsdbtCoderate::CODERATE_2_3
                | FrontendIsdbtCoderate::CODERATE_3_4
                | FrontendIsdbtCoderate::CODERATE_5_6
                | FrontendIsdbtCoderate::CODERATE_7_8
        ) {
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::UnsupportedBandwidth,
                "unsupported ISDB-T layer coderate",
            ));
        }
        if !matches!(
            layer.timeInterleave,
            FrontendIsdbtTimeInterleaveMode::AUTO
                | FrontendIsdbtTimeInterleaveMode::INTERLEAVE_3_0
                | FrontendIsdbtTimeInterleaveMode::INTERLEAVE_3_1
                | FrontendIsdbtTimeInterleaveMode::INTERLEAVE_3_2
                | FrontendIsdbtTimeInterleaveMode::INTERLEAVE_3_4
        ) {
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::UnsupportedBandwidth,
                "unsupported ISDB-T layer time interleave",
            ));
        }
    }
    Ok(())
}

fn validate_isdbs_fixed_settings(
    s: &android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::FrontendIsdbsSettings::FrontendIsdbsSettings,
) -> Result<(), HalError> {
    if !matches!(
        s.modulation,
        FrontendIsdbsModulation::AUTO
            | FrontendIsdbsModulation::MOD_BPSK
            | FrontendIsdbsModulation::MOD_QPSK
            | FrontendIsdbsModulation::MOD_TC8PSK
    ) {
        return Err(HalError::invalid_argument(
            HalInvalidArgumentKind::UnsupportedBandwidth,
            "unsupported ISDB-S modulation",
        ));
    }
    if !matches!(
        s.coderate,
        FrontendIsdbsCoderate::AUTO
            | FrontendIsdbsCoderate::CODERATE_1_2
            | FrontendIsdbsCoderate::CODERATE_2_3
            | FrontendIsdbsCoderate::CODERATE_3_4
            | FrontendIsdbsCoderate::CODERATE_5_6
            | FrontendIsdbsCoderate::CODERATE_7_8
    ) {
        return Err(HalError::invalid_argument(
            HalInvalidArgumentKind::UnsupportedSymbolRate,
            "unsupported ISDB-S coderate",
        ));
    }
    if s.symbolRate != 0 {
        return Err(HalError::invalid_argument(
            HalInvalidArgumentKind::UnsupportedSymbolRate,
            "ISDB-S symbolRate must be 0 in this product scope",
        ));
    }
    Ok(())
}

fn map_isdbs_stream_selector(
    stream_id: i32,
    stream_id_type: FrontendIsdbsStreamIdType,
    frequency_hz: u64,
) -> Result<(Option<u32>, Option<FrontendStreamIdKind>), HalError> {
    match stream_id_type {
        FrontendIsdbsStreamIdType::UNDEFINED => {
            if stream_id != 0 {
                return Err(HalError::invalid_argument(
                    HalInvalidArgumentKind::UnsupportedStreamSelector,
                    "streamId must be 0 when streamIdType is UNDEFINED",
                ));
            }
            Ok((None, None))
        }
        FrontendIsdbsStreamIdType::STREAM_ID => {
            if stream_id == AOSP_TUNER_INVALID_STREAM_ID {
                return Ok((None, None));
            }
            if stream_id < 0 {
                return Err(HalError::invalid_argument(
                    HalInvalidArgumentKind::InvalidStreamIdRange,
                    "negative ISDB-S stream selector",
                ));
            }
            if is_japan_cs110_if_frequency_hz(frequency_hz) {
                return Err(HalError::invalid_argument(
                    HalInvalidArgumentKind::UnsupportedStreamSelector,
                    "CS110 tune must not carry TSID or relative stream selector",
                ));
            }
            let value = u32::try_from(stream_id).map_err(|_| {
                HalError::invalid_argument(
                    HalInvalidArgumentKind::InvalidStreamIdRange,
                    "ISDB-S stream selector out of range",
                )
            })?;
            Ok((Some(value), Some(FrontendStreamIdKind::AbsoluteStreamId)))
        }
        FrontendIsdbsStreamIdType::RELATIVE_STREAM_NUMBER => {
            if stream_id < 0 {
                return Err(HalError::invalid_argument(
                    HalInvalidArgumentKind::InvalidStreamIdRange,
                    "negative ISDB-S relative stream selector",
                ));
            }
            if is_japan_cs110_if_frequency_hz(frequency_hz) {
                return Err(HalError::invalid_argument(
                    HalInvalidArgumentKind::UnsupportedStreamSelector,
                    "CS110 tune must not carry TSID or relative stream selector",
                ));
            }
            let value = u32::try_from(stream_id).map_err(|_| {
                HalError::invalid_argument(
                    HalInvalidArgumentKind::InvalidStreamIdRange,
                    "ISDB-S relative stream selector out of range",
                )
            })?;
            Ok((
                Some(value),
                Some(FrontendStreamIdKind::RelativeStreamNumber),
            ))
        }
        _ => Err(HalError::invalid_argument(
            HalInvalidArgumentKind::UnsupportedStreamSelector,
            "unsupported ISDB-S streamIdType",
        )),
    }
}

fn aidl_frontend_settings_to_request(
    settings: &FrontendSettings,
) -> Result<FrontendTuneRequest, HalError> {
    match settings {
        FrontendSettings::Isdbt(s) => {
            validate_isdbt_fixed_settings(s)?;
            Ok(FrontendTuneRequest {
                system: FrontendSystem::IsdbT,
                frequency: cast_u64_field(s.frequency, "isdbt.frequency")?,
                end_frequency: optional_positive_i64_to_u64_field(
                    s.endFrequency,
                    "isdbt.endFrequency",
                )?,
                stream_id: None,
                stream_id_kind: None,
                bandwidth_hz: map_isdbt_bandwidth(s.bandwidth),
                symbol_rate: None,
            })
        }
        FrontendSettings::Isdbs(s) => {
            validate_isdbs_fixed_settings(s)?;
            let frequency = cast_u64_field(s.frequency, "isdbs.frequency")?;
            let (stream_id, stream_id_kind) =
                map_isdbs_stream_selector(s.streamId, s.streamIdType, frequency)?;
            Ok(FrontendTuneRequest {
                system: FrontendSystem::IsdbS,
                frequency,
                end_frequency: optional_positive_i64_to_u64_field(
                    s.endFrequency,
                    "isdbs.endFrequency",
                )?,
                stream_id,
                stream_id_kind,
                bandwidth_hz: None,
                symbol_rate: None,
            })
        }
        FrontendSettings::Isdbs3(_) => Err(HalError::Unsupported(
            "ISDB-S3 is outside the r51 product scope",
        )),
        FrontendSettings::Dvbs(_) => Err(HalError::Unsupported(
            "DVB-S is outside the r51 product scope",
        )),
        _ => Err(HalError::Unsupported(
            "frontend setting is outside the r51 product scope",
        )),
    }
}

fn validate_frontend_request_against_entry(
    entry: &FrontendRegistryEntry,
    request: &FrontendTuneRequest,
) -> Result<(), HalError> {
    if entry.system != request.system {
        return Err(HalError::invalid_argument(
            HalInvalidArgumentKind::MissingDeliverySystem,
            format!(
                "requested frontend system {} does not match exported frontend {}",
                request.system.as_hint(),
                entry.system.as_hint()
            ),
        ));
    }

    match request.system {
        FrontendSystem::IsdbT => {
            if !is_japan_isdbt_frequency_contract_hz(request.frequency) {
                return Err(HalError::invalid_argument(
                    HalInvalidArgumentKind::UnsupportedFrequency,
                    "ISDB-T frequency is outside Japan CATV C13..UHF62 contract range",
                ));
            }
            if let Some(end_frequency) = request.end_frequency {
                if end_frequency < request.frequency
                    || !is_japan_isdbt_frequency_contract_hz(end_frequency)
                {
                    return Err(HalError::invalid_argument(
                        HalInvalidArgumentKind::UnsupportedScanRange,
                        "ISDB-T endFrequency must be >= frequency and inside Japan contract range",
                    ));
                }
            }
            if request.stream_id.is_some() || request.stream_id_kind.is_some() {
                return Err(HalError::invalid_argument(
                    HalInvalidArgumentKind::UnsupportedStreamSelector,
                    "ISDB-T tune must not carry ISDB-S stream selector",
                ));
            }
        }
        FrontendSystem::IsdbS => {
            let is_bs = is_japan_bs_if_frequency_hz(request.frequency);
            let is_cs110 = is_japan_cs110_if_frequency_hz(request.frequency);
            if !is_bs && !is_cs110 {
                return Err(HalError::invalid_argument(
                    HalInvalidArgumentKind::UnsupportedFrequency,
                    "ISDB-S frequency must be a Japan BS/CS110 IF center frequency",
                ));
            }
            if let Some(end_frequency) = request.end_frequency {
                if end_frequency < request.frequency {
                    return Err(HalError::invalid_argument(
                        HalInvalidArgumentKind::UnsupportedScanRange,
                        "ISDB-S endFrequency must be >= frequency",
                    ));
                }
                if !(is_japan_bs_if_frequency_hz(end_frequency)
                    || is_japan_cs110_if_frequency_hz(end_frequency))
                {
                    return Err(HalError::invalid_argument(
                        HalInvalidArgumentKind::UnsupportedScanRange,
                        "ISDB-S endFrequency must be a Japan BS/CS110 IF center frequency",
                    ));
                }
            }
            if is_cs110 && (request.stream_id.is_some() || request.stream_id_kind.is_some()) {
                return Err(HalError::invalid_argument(
                    HalInvalidArgumentKind::UnsupportedStreamSelector,
                    "CS110 tune must not carry TSID or relative stream selector",
                ));
            }
        }
        FrontendSystem::IsdbS3 | FrontendSystem::DvbS => {
            return Err(HalError::Unsupported(
                "frontend system is outside the r51 product scope",
            ));
        }
    }
    Ok(())
}

fn validate_frontend_request_for_object(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    request: &FrontendTuneRequest,
) -> BinderResult<FrontendRegistryEntry> {
    let entry = runtime_frontend_entry_for_object(runtime, handle)?;
    validate_frontend_request_against_entry(&entry, request).map_err(status_from_hal_error)?;
    validate_frontend_lnb_candidate(runtime, &entry, request)?;
    Ok(entry)
}

fn validate_frontend_lnb_candidate(
    runtime: &SharedTunerRuntime,
    entry: &FrontendRegistryEntry,
    request: &FrontendTuneRequest,
) -> BinderResult<()> {
    if !matches!(request.system, FrontendSystem::IsdbS) {
        return Ok(());
    }
    let lnb = runtime
        .lock()
        .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
        .lnb_for_frontend_id(entry.id.0);
    match (entry.lnb_profile, lnb) {
        (Some(expected_profile), Some(lnb_entry)) if lnb_entry.profile == expected_profile => {
            Ok(())
        }
        (Some(_), Some(_)) => Err(status_unknown_error(
            "frontend/LNB profile mismatch in runtime registry",
        )),
        _ => Err(service_error(
            TunerResult::UNAVAILABLE.0,
            "ISDB-S frontend does not have a registered LNB candidate",
        )),
    }
}

fn validate_backend_tune_preflight(
    entry: &FrontendRegistryEntry,
    request: &FrontendTuneRequest,
) -> Result<(), HalError> {
    match entry.backend {
        FrontendBackendKind::Px4CharDevice => {
            let _mapped = maleicacid_tuner_hal2_device::px4::map_tune_request_to_px4(request)?;
        }
        FrontendBackendKind::LinuxDvb => {
            let normalized =
                maleicacid_tuner_hal2_device::dvb::normalized_tune_request_from_common(request)?;
            let pairs = maleicacid_tuner_hal2_device::dvb::tune_property_pairs(&normalized)?;
            let _dtv_properties = pairs.to_dtv_properties();
        }
    }
    Ok(())
}

fn backend_scan_candidates(
    entry: &FrontendRegistryEntry,
    request: &FrontendTuneRequest,
    scan_mode: FrontendScanMode,
) -> Result<Vec<FrontendTuneRequest>, HalError> {
    let candidates = match entry.backend {
        FrontendBackendKind::Px4CharDevice => {
            maleicacid_tuner_hal2_device::px4::px4_scan_requests(request)?
        }
        FrontendBackendKind::LinuxDvb => {
            maleicacid_tuner_hal2_device::dvb::dvb_scan_requests(request, scan_mode)?
        }
    };
    for candidate in candidates.iter() {
        validate_backend_tune_preflight(entry, candidate)?;
    }
    Ok(candidates)
}

fn mark_frontend_callback_unhealthy(runtime: &SharedTunerRuntime, handle: AidlObjectHandle) {
    if let Ok(mut guard) = runtime.lock() {
        guard.callback_registry_mut().mark_unhealthy(
            handle.object_kind(),
            handle.object_id(),
            handle.generation(),
            AidlApi::FrontendSetCallback,
        );
    }
}

fn mark_scan_end_callback_failed(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    frontend_id: i32,
    generation: u64,
) -> Result<(), HalError> {
    mark_frontend_callback_unhealthy(runtime, handle);
    let mut guard = runtime.lock().map_err(|_| {
        HalError::internal(
            HalInternalKind::InvariantViolation,
            "service runtime lock poisoned while marking scan callback failure",
        )
    })?;
    guard.mark_frontend_scan_session_callback_failed(frontend_id, generation)
}

fn mark_tune_worker_failed(
    runtime: &SharedTunerRuntime,
    frontend_id: i32,
    generation: u64,
    error: HalError,
) -> Result<(), HalError> {
    let mut guard = runtime.lock().map_err(|_| {
        HalError::internal(
            HalInternalKind::InvariantViolation,
            "service runtime lock poisoned while marking tune worker failure",
        )
    })?;
    guard.mark_frontend_tune_worker_failed(frontend_id, generation, error)
}

fn deliver_scan_end_callback(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    frontend_id: i32,
    generation: u64,
) -> Result<(), HalError> {
    let callback = match frontend_callback_for_owner(handle) {
        Ok(Some(callback)) => callback,
        Ok(None) => {
            if let Err(mark_error) =
                mark_scan_end_callback_failed(runtime, handle, frontend_id, generation)
            {
                return Err(mark_error);
            }
            return Err(HalError::callback_failed(
                "IFrontendCallback.onScanMessage(END)",
                "frontend callback is not registered",
            ));
        }
        Err(_) => {
            if let Err(mark_error) =
                mark_scan_end_callback_failed(runtime, handle, frontend_id, generation)
            {
                return Err(mark_error);
            }
            return Err(HalError::callback_failed(
                "IFrontendCallback.onScanMessage(END)",
                "callback store lock poisoned",
            ));
        }
    };
    let message = FrontendScanMessage::IsEnd(true);
    if let Err(err) = callback.onScanMessage(FrontendScanMessageType::END, &message) {
        if let Err(mark_error) =
            mark_scan_end_callback_failed(runtime, handle, frontend_id, generation)
        {
            return Err(mark_error);
        }
        return Err(HalError::callback_failed(
            "IFrontendCallback.onScanMessage(END)",
            format!("binder failure: {err:?}"),
        ));
    }
    Ok(())
}

fn map_frontend_worker_start_error(error: FrontendWorkerStartError) -> Status {
    match error {
        FrontendWorkerStartError::AlreadyRunning { .. } => service_error(
            TunerResult::INVALID_STATE.0,
            "frontend worker is already running",
        ),
        FrontendWorkerStartError::SpawnFailed { detail } => service_error(
            TunerResult::UNKNOWN_ERROR.0,
            &format!("frontend worker spawn failed: {detail}"),
        ),
    }
}

fn start_frontend_backend_tune_worker_for_object(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    entry: &FrontendRegistryEntry,
    request: FrontendTuneRequest,
    kind: FrontendWorkerKind,
) -> BinderResult<()> {
    let frontend_id = runtime_entry_public_id(runtime, handle, AidlObjectKind::Frontend)?;
    let plan = FrontendBackendTunePlan::new(
        frontend_id,
        entry.backend,
        FrontendDevicePath::new(entry.device_path.clone()),
        request.clone(),
    );
    let mut guard = runtime
        .lock()
        .map_err(|_| status_unknown_error("service runtime lock poisoned"))?;
    if guard
        .frontend_has_same_active_tune(frontend_id, &request)
        .map_err(status_from_hal_error)?
    {
        return Ok(());
    }
    let snapshot = guard
        .frontend_runtime_snapshot(frontend_id)
        .map_err(status_from_hal_error)?;
    let demux_snapshots = guard
        .bound_demux_runtime_snapshots(frontend_id)
        .map_err(status_from_hal_error)?;
    let generation = guard
        .prepare_frontend_worker_generation(frontend_id, kind)
        .map_err(status_from_hal_error)?;
    if let Err(error) = guard.reset_bound_demuxes_for_frontend_tune_start(frontend_id) {
        guard
            .restore_frontend_runtime_snapshot(frontend_id, snapshot)
            .map_err(status_from_hal_error)?;
        guard
            .restore_bound_demux_runtime_snapshots(demux_snapshots.clone())
            .map_err(status_from_hal_error)?;
        return Err(status_from_hal_error(error));
    }
    if let Err(error) =
        guard.install_frontend_live_reader_descriptor_for_generation(frontend_id, kind, generation)
    {
        guard
            .restore_frontend_runtime_snapshot(frontend_id, snapshot)
            .map_err(status_from_hal_error)?;
        guard
            .restore_bound_demux_runtime_snapshots(demux_snapshots.clone())
            .map_err(status_from_hal_error)?;
        return Err(status_from_hal_error(error));
    }
    let previous_tune_for_worker = snapshot.active_tune_request.clone();
    let frontend_snapshot_for_worker = snapshot.clone();
    let demux_snapshots_for_worker = demux_snapshots.clone();
    let runtime_for_worker = Arc::clone(runtime);
    if let Err(error) = guard.start_frontend_worker(frontend_id, kind, generation, move |ctx| {
        let session = match FrontendBackendSession::open_and_submit_with_previous_report(
            &plan,
            previous_tune_for_worker,
        ) {
            Ok(session) => session,
            Err(failure) if failure.rollback_succeeded => {
                let report_error = failure.error;
                let mut guard = runtime_for_worker.lock().map_err(|_| {
                    HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "service runtime lock poisoned while restoring tune rollback state",
                    )
                })?;
                guard.restore_frontend_runtime_snapshot(
                    frontend_id,
                    frontend_snapshot_for_worker.clone(),
                )?;
                guard.restore_bound_demux_runtime_snapshots(demux_snapshots_for_worker.clone())?;
                return Err(report_error);
            }
            Err(failure) => {
                let report_error = failure.error.clone();
                match mark_tune_worker_failed(
                    &runtime_for_worker,
                    frontend_id,
                    generation,
                    failure.error,
                ) {
                    Ok(()) => return Err(report_error),
                    Err(mark_error) => return Err(mark_error),
                }
            }
        };
        {
            let mut guard = runtime_for_worker.lock().map_err(|_| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "service runtime lock poisoned while recording frontend signal state",
                )
            })?;
            guard.record_frontend_signal_state(
                frontend_id,
                generation,
                session.initial_signal_state(),
            )?;
        }
        let mut live_pump = None;
        while !ctx.cancel_requested() {
            if live_pump.is_none() {
                let live_reader_descriptor = {
                    let guard = runtime_for_worker.lock().map_err(|_| {
                        HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "service runtime lock poisoned while checking frontend live pump readiness",
                    )
                    })?;
                    guard.frontend_live_reader_descriptor_for_live_pump(frontend_id)?
                };
                if let Some(descriptor) = live_reader_descriptor {
                    let reader = session.open_live_reader(&descriptor)?;
                    live_pump = Some(start_frontend_demux_live_pump_from_reader(
                        Arc::clone(&runtime_for_worker),
                        frontend_id,
                        reader,
                    )?);
                }
            }
            if let Some(owner) = live_pump.as_mut() {
                match owner.collect_if_finished() {
                    FrontendLivePumpJoinOutcome::Running => {}
                    FrontendLivePumpJoinOutcome::Completed(result) => {
                        result?;
                        return session.stop();
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        if let Some(owner) = live_pump {
            let _ = owner.join_after_stop()?;
        }
        session.stop()
    }) {
        guard
            .restore_frontend_runtime_snapshot(frontend_id, snapshot)
            .map_err(status_from_hal_error)?;
        guard
            .restore_bound_demux_runtime_snapshots(demux_snapshots.clone())
            .map_err(status_from_hal_error)?;
        return Err(map_frontend_worker_start_error(error));
    }
    guard
        .commit_frontend_active_tune_request(frontend_id, generation, request)
        .map_err(status_from_hal_error)
}

fn run_frontend_backend_scan_session_worker(
    runtime: SharedTunerRuntime,
    handle: AidlObjectHandle,
    ctx: FrontendWorkerContext,
    backend: FrontendBackendKind,
    device_path: FrontendDevicePath,
    candidates: Vec<FrontendTuneRequest>,
    previous_request: Option<FrontendTuneRequest>,
    frontend_snapshot: maleicacid_tuner_hal2_device::FrontendRuntimeSnapshot,
    demux_snapshots: Vec<(
        maleicacid_tuner_hal2_service_runtime::DemuxRuntimeId,
        maleicacid_tuner_hal2_demux::runtime::DemuxRuntimeSnapshot,
    )>,
) -> Result<(), HalError> {
    for candidate in candidates {
        if ctx.cancel_requested() {
            return Ok(());
        }
        let plan = FrontendBackendTunePlan::new(
            ctx.frontend_id(),
            backend,
            device_path.clone(),
            candidate,
        );
        let session = match FrontendBackendSession::open_and_submit_with_previous_report(
            &plan,
            previous_request.clone(),
        ) {
            Ok(session) => session,
            Err(failure) if failure.rollback_succeeded => {
                let mut guard = runtime.lock().map_err(|_| {
                    HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "service runtime lock poisoned while restoring scan rollback state",
                    )
                })?;
                guard.restore_frontend_runtime_snapshot(
                    ctx.frontend_id(),
                    frontend_snapshot.clone(),
                )?;
                guard.restore_bound_demux_runtime_snapshots(demux_snapshots.clone())?;
                return Err(failure.error);
            }
            Err(failure) => {
                let mut guard = runtime.lock().map_err(|_| {
                    HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "service runtime lock poisoned while marking scan backend failure",
                    )
                })?;
                guard.mark_frontend_scan_session_backend_failed(
                    ctx.frontend_id(),
                    ctx.generation(),
                )?;
                return Err(failure.error);
            }
        };
        {
            let mut guard = runtime.lock().map_err(|_| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "service runtime lock poisoned while recording scan signal state",
                )
            })?;
            guard.record_frontend_signal_state(
                ctx.frontend_id(),
                ctx.generation(),
                session.initial_signal_state(),
            )?;
        }
        for _ in 0..5 {
            if ctx.cancel_requested() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        session.stop()?;
        if ctx.cancel_requested() {
            return Ok(());
        }
        let mut guard = runtime.lock().map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "service runtime lock poisoned while advancing scan session",
            )
        })?;
        let has_next = guard
            .advance_frontend_scan_session_after_candidate(ctx.frontend_id(), ctx.generation())?;
        drop(guard);
        if !has_next {
            deliver_scan_end_callback(&runtime, handle, ctx.frontend_id(), ctx.generation())?;
            return Ok(());
        }
    }
    Ok(())
}

fn start_frontend_backend_scan_session_for_object(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    entry: &FrontendRegistryEntry,
    request: FrontendTuneRequest,
    scan_mode: FrontendScanMode,
    candidates: Vec<FrontendTuneRequest>,
) -> BinderResult<()> {
    replace_existing_scan_worker_for_object(runtime, handle)?;
    let frontend_id = runtime_entry_public_id(runtime, handle, AidlObjectKind::Frontend)?;
    let fingerprint = format!("{:?}:{:?}", scan_mode, request);
    let mut guard = runtime
        .lock()
        .map_err(|_| status_unknown_error("service runtime lock poisoned"))?;
    let snapshot = guard
        .frontend_runtime_snapshot(frontend_id)
        .map_err(status_from_hal_error)?;
    let demux_snapshots = guard
        .bound_demux_runtime_snapshots(frontend_id)
        .map_err(status_from_hal_error)?;
    let generation = guard
        .prepare_frontend_worker_generation(frontend_id, FrontendWorkerKind::Scan)
        .map_err(status_from_hal_error)?;
    if let Err(error) = guard.reset_bound_demuxes_for_frontend_tune_start(frontend_id) {
        guard
            .restore_frontend_runtime_snapshot(frontend_id, snapshot)
            .map_err(status_from_hal_error)?;
        guard
            .restore_bound_demux_runtime_snapshots(demux_snapshots.clone())
            .map_err(status_from_hal_error)?;
        return Err(status_from_hal_error(error));
    }
    if let Err(error) = guard.install_frontend_live_reader_descriptor_for_generation(
        frontend_id,
        FrontendWorkerKind::Scan,
        generation,
    ) {
        guard
            .restore_frontend_runtime_snapshot(frontend_id, snapshot)
            .map_err(status_from_hal_error)?;
        guard
            .restore_bound_demux_runtime_snapshots(demux_snapshots.clone())
            .map_err(status_from_hal_error)?;
        return Err(status_from_hal_error(error));
    }
    if let Err(error) =
        guard.begin_frontend_scan_session(frontend_id, generation, fingerprint, candidates.clone())
    {
        guard
            .restore_frontend_runtime_snapshot(frontend_id, snapshot)
            .map_err(status_from_hal_error)?;
        guard
            .restore_bound_demux_runtime_snapshots(demux_snapshots.clone())
            .map_err(status_from_hal_error)?;
        return Err(status_from_hal_error(error));
    }
    let previous_tune_for_worker = snapshot.active_tune_request.clone();
    let frontend_snapshot_for_worker = snapshot.clone();
    let demux_snapshots_for_worker = demux_snapshots.clone();
    let runtime_for_worker = Arc::clone(runtime);
    let backend = entry.backend;
    let device_path = FrontendDevicePath::new(entry.device_path.clone());
    if let Err(error) = guard.start_frontend_worker(
        frontend_id,
        FrontendWorkerKind::Scan,
        generation,
        move |ctx| {
            run_frontend_backend_scan_session_worker(
                runtime_for_worker,
                handle,
                ctx,
                backend,
                device_path,
                candidates,
                previous_tune_for_worker,
                frontend_snapshot_for_worker,
                demux_snapshots_for_worker,
            )
        },
    ) {
        guard
            .restore_frontend_runtime_snapshot(frontend_id, snapshot)
            .map_err(status_from_hal_error)?;
        guard
            .restore_bound_demux_runtime_snapshots(demux_snapshots.clone())
            .map_err(status_from_hal_error)?;
        return Err(map_frontend_worker_start_error(error));
    }
    Ok(())
}

fn request_frontend_worker_stop_and_join_for_object(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    kind: FrontendWorkerKind,
    reason: FrontendWorkerCancelReason,
) -> BinderResult<FrontendWorkerStopOutcome> {
    let frontend_id = runtime_entry_public_id(runtime, handle, AidlObjectKind::Frontend)?;
    let outcome = runtime
        .lock()
        .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
        .request_frontend_worker_stop_and_join(frontend_id, kind, reason);
    if let FrontendWorkerStopOutcome::Completed {
        result: Err(error), ..
    } = &outcome
    {
        return Err(status_from_hal_error(error.clone()));
    }
    Ok(outcome)
}

fn record_scan_cancelled_terminal_event_for_object(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    generation: u64,
    reason: FrontendWorkerCancelReason,
) -> BinderResult<()> {
    let frontend_id = runtime_entry_public_id(runtime, handle, AidlObjectKind::Frontend)?;
    runtime
        .lock()
        .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
        .cancel_frontend_scan_session(frontend_id, generation, reason)
        .map_err(status_from_hal_error)
}

fn record_scan_cancelled_from_stop_outcome(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    outcome: &FrontendWorkerStopOutcome,
    reason: FrontendWorkerCancelReason,
) -> BinderResult<()> {
    let generation = match outcome {
        FrontendWorkerStopOutcome::NotRunning => return Ok(()),
        FrontendWorkerStopOutcome::CancelRequested { generation, .. }
        | FrontendWorkerStopOutcome::Completed {
            generation,
            result: Ok(()),
            ..
        } => *generation,
        FrontendWorkerStopOutcome::Completed {
            result: Err(error), ..
        } => {
            return Err(status_from_hal_error(error.clone()));
        }
    };
    record_scan_cancelled_terminal_event_for_object(runtime, handle, generation, reason)
}

fn replace_existing_scan_worker_for_object(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
) -> BinderResult<()> {
    let reason = FrontendWorkerCancelReason::SupersededByNewRequest;
    let outcome = request_frontend_worker_stop_and_join_for_object(
        runtime,
        handle,
        FrontendWorkerKind::Scan,
        reason,
    )?;
    record_scan_cancelled_from_stop_outcome(runtime, handle, &outcome, reason)?;
    if !matches!(outcome, FrontendWorkerStopOutcome::NotRunning) {
        clear_frontend_live_reader_descriptor_for_object(runtime, handle)?;
    }
    Ok(())
}

fn clear_frontend_live_reader_descriptor_for_object(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
) -> BinderResult<()> {
    let frontend_id = runtime_entry_public_id(runtime, handle, AidlObjectKind::Frontend)?;
    runtime
        .lock()
        .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
        .clear_frontend_live_reader_descriptor_and_idle(frontend_id)
        .map_err(status_from_hal_error)
}

fn stop_frontend_live_data_and_unbind_for_object(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
) -> BinderResult<()> {
    let frontend_id = runtime_entry_public_id(runtime, handle, AidlObjectKind::Frontend)?;
    runtime
        .lock()
        .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
        .stop_frontend_live_data_and_unbind(frontend_id)
        .map(|_| ())
        .map_err(status_from_hal_error)
}

fn close_frontend_live_data_and_unbind_for_object(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
) -> BinderResult<()> {
    let frontend_id = runtime_entry_public_id(runtime, handle, AidlObjectKind::Frontend)?;
    runtime
        .lock()
        .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
        .close_frontend_live_data_and_unbind(frontend_id)
        .map(|_| ())
        .map_err(status_from_hal_error)
}

fn request_all_frontend_workers_stop_for_object(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    reason: FrontendWorkerCancelReason,
) -> BinderResult<()> {
    let tune_outcome = request_frontend_worker_stop_and_join_for_object(
        runtime,
        handle,
        FrontendWorkerKind::Tune,
        reason,
    );
    let scan_outcome = request_frontend_worker_stop_and_join_for_object(
        runtime,
        handle,
        FrontendWorkerKind::Scan,
        reason,
    );

    let mut first_error = None;
    if let Ok(outcome) = &scan_outcome {
        if let Err(error) =
            record_scan_cancelled_from_stop_outcome(runtime, handle, outcome, reason)
        {
            first_error = Some(error);
        }
    }
    if let Err(error) = tune_outcome {
        if first_error.is_none() {
            first_error = Some(error);
        }
    }
    if let Err(error) = scan_outcome {
        if first_error.is_none() {
            first_error = Some(error);
        }
    }
    if let Some(error) = first_error {
        Err(error)
    } else {
        Ok(())
    }
}

fn aidl_scan_type_to_mode(scan_type: FrontendScanType) -> Result<FrontendScanMode, HalError> {
    match scan_type {
        FrontendScanType::SCAN_AUTO => Ok(FrontendScanMode::Auto),
        FrontendScanType::SCAN_BLIND => Err(HalError::Unsupported(
            "blind scan is outside the r51 product scope; TIS must submit explicit candidates",
        )),
        FrontendScanType::SCAN_UNDEFINED => Err(HalError::invalid_argument(
            HalInvalidArgumentKind::NumericRange,
            "scan type must be SCAN_AUTO or SCAN_BLIND",
        )),
        _ => Err(HalError::Unsupported(
            "frontend scan type is outside the r51 product scope",
        )),
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
    input: Option<AidlInputSnapshot>,
) -> AidlMethodCall {
    AidlMethodCall::UnsupportedPublicApi { object, api, input }
}

fn unavailable_after_tuner_method_plan(
    service: &TunerAidlService,
    api: AidlApi,
    input: Option<AidlInputSnapshot>,
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
    input: Option<AidlInputSnapshot>,
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

fn snapshot_value(source_type: &'static str, value: impl Into<String>) -> AidlInputSnapshot {
    AidlInputSnapshot::single_field(source_type, "value", value.into())
}

fn snapshot_fields(source_type: &'static str, fields: Vec<AidlInputField>) -> AidlInputSnapshot {
    AidlInputSnapshot::from_fields(source_type, fields)
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

fn register_child_aidl_object(
    runtime: &SharedTunerRuntime,
    kind: AidlObjectKind,
    public_runtime_id: i32,
    owner: RuntimeOwnerRelation,
) -> BinderResult<AidlObjectHandle> {
    let entry = runtime
        .lock()
        .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
        .register_aidl_object_for_runtime_auto_generation(kind, i64::from(public_runtime_id), owner)
        .map_err(|_| {
            service_error(
                TunerResult::INVALID_STATE.0,
                "AIDL child object registration failed",
            )
        })?;
    Ok(AidlObjectHandle::new(
        entry.object_kind,
        entry.object_id,
        entry.generation,
    ))
}

fn rollback_child_aidl_object(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
) -> BinderResult<()> {
    runtime
        .lock()
        .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
        .unregister_aidl_object_after_registration_failure(handle.object_id(), handle.generation())
        .map(|_| ())
        .map_err(|_| {
            service_error(
                TunerResult::UNKNOWN_ERROR.0,
                "AIDL child object rollback failed",
            )
        })
}

fn unregister_child_public_runtime(
    runtime: &SharedTunerRuntime,
    kind: AidlObjectKind,
    public_runtime_id: i32,
) -> BinderResult<()> {
    let mut runtime = runtime
        .lock()
        .map_err(|_| status_unknown_error("service runtime lock poisoned"))?;
    match kind {
        AidlObjectKind::Filter => {
            runtime.unregister_filter_runtime(public_runtime_id);
        }
        AidlObjectKind::Dvr => {
            runtime.unregister_dvr_runtime(public_runtime_id);
        }
        AidlObjectKind::Descrambler => {
            runtime.unregister_descrambler_runtime(public_runtime_id);
        }
        _ => {}
    }
    Ok(())
}

fn allocate_filter_public_runtime(
    runtime: &SharedTunerRuntime,
    owner_demux_id: i32,
) -> BinderResult<i32> {
    let entry = runtime
        .lock()
        .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
        .allocate_filter_runtime(owner_demux_id)
        .map_err(|_| status_unknown_error("filter runtime allocation failed"))?;
    Ok(entry.id.0)
}

fn allocate_dvr_public_runtime(
    runtime: &SharedTunerRuntime,
    owner_demux_id: i32,
) -> BinderResult<i32> {
    let entry = runtime
        .lock()
        .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
        .allocate_dvr_runtime(owner_demux_id)
        .map_err(|_| status_unknown_error("DVR runtime allocation failed"))?;
    Ok(entry.id.0)
}

fn open_kind_for_demux_filter_type(filter_type: &DemuxFilterType) -> PipelineOpenKind {
    if filter_type.mainType != DemuxFilterMainType::TS {
        return PipelineOpenKind::Other;
    }
    match &filter_type.subType {
        DemuxFilterSubType::TsFilterType(ts_type) => match *ts_type {
            DemuxTsFilterType::TS => PipelineOpenKind::Raw,
            DemuxTsFilterType::SECTION => PipelineOpenKind::Section,
            DemuxTsFilterType::PES => PipelineOpenKind::Pes,
            DemuxTsFilterType::RECORD => PipelineOpenKind::Record,
            DemuxTsFilterType::AUDIO | DemuxTsFilterType::VIDEO => PipelineOpenKind::Av,
            _ => PipelineOpenKind::Other,
        },
        _ => PipelineOpenKind::Other,
    }
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
        let snapshot = snapshot_fields(
            "IFrontendCallback",
            vec![AidlInputField::new("strong_present", "true")],
        );
        self.plan_method(AidlMethodCall::FrontendSetCallback(snapshot))?;
        self.retain_callback(callback)
    }
    fn tune(&self, settings: &FrontendSettings) -> BinderResult<()> {
        self.ensure_open()?;
        let request = aidl_frontend_settings_to_request(settings).map_err(status_from_hal_error)?;
        let entry = validate_frontend_request_for_object(&self.runtime(), self.handle(), &request)?;
        validate_backend_tune_preflight(&entry, &request).map_err(status_from_hal_error)?;
        self.plan_method(AidlMethodCall::FrontendTune(request.clone()))?;
        start_frontend_backend_tune_worker_for_object(
            &self.runtime(),
            self.handle(),
            &entry,
            request,
            FrontendWorkerKind::Tune,
        )
    }
    fn stopTune(&self) -> BinderResult<()> {
        self.ensure_open()?;
        self.plan_method(AidlMethodCall::FrontendStopTune)?;
        request_frontend_worker_stop_and_join_for_object(
            &self.runtime(),
            self.handle(),
            FrontendWorkerKind::Tune,
            FrontendWorkerCancelReason::StopRequested,
        )?;
        stop_frontend_live_data_and_unbind_for_object(&self.runtime(), self.handle())
    }
    fn close(&self) -> BinderResult<()> {
        self.ensure_open()?;
        self.plan_method(AidlMethodCall::FrontendClose)?;
        request_all_frontend_workers_stop_for_object(
            &self.runtime(),
            self.handle(),
            FrontendWorkerCancelReason::FrontendClosing,
        )?;
        close_frontend_live_data_and_unbind_for_object(&self.runtime(), self.handle())?;
        self.close_object()
    }
    fn scan(&self, settings: &FrontendSettings, scan_type: FrontendScanType) -> BinderResult<()> {
        self.ensure_open()?;
        let scan_mode = aidl_scan_type_to_mode(scan_type).map_err(status_from_hal_error)?;
        let request = aidl_frontend_settings_to_request(settings).map_err(status_from_hal_error)?;
        let entry = validate_frontend_request_for_object(&self.runtime(), self.handle(), &request)?;
        let candidates =
            backend_scan_candidates(&entry, &request, scan_mode).map_err(status_from_hal_error)?;
        self.plan_method(AidlMethodCall::FrontendScan(request.clone()))?;
        start_frontend_backend_scan_session_for_object(
            &self.runtime(),
            self.handle(),
            &entry,
            request,
            scan_mode,
            candidates,
        )
    }
    fn stopScan(&self) -> BinderResult<()> {
        self.ensure_open()?;
        self.plan_method(AidlMethodCall::FrontendStopScan)?;
        let reason = FrontendWorkerCancelReason::StopRequested;
        let outcome = request_frontend_worker_stop_and_join_for_object(
            &self.runtime(),
            self.handle(),
            FrontendWorkerKind::Scan,
            reason,
        )?;
        record_scan_cancelled_from_stop_outcome(&self.runtime(), self.handle(), &outcome, reason)?;
        clear_frontend_live_reader_descriptor_for_object(&self.runtime(), self.handle())
    }
    fn getStatus(&self, status_types: &[FrontendStatusType]) -> BinderResult<Vec<FrontendStatus>> {
        self.ensure_open()?;
        self.plan_method(unsupported_public_api_call(
            AidlObjectKind::Frontend,
            AidlApi::FrontendGetStatus,
            Some(snapshot_fields(
                "FrontendStatusTypes",
                vec![AidlInputField::new("count", status_types.len().to_string())],
            )),
        ))?;
        let entry = runtime_frontend_entry_for_object(&self.runtime(), self.handle())?;
        let signal_state = frontend_signal_state_for_object(&self.runtime(), self.handle())?;
        frontend_status_for_types(&entry, signal_state, status_types)
    }
    fn setLnb(&self, lnb_id: i32) -> BinderResult<()> {
        self.ensure_open()?;
        let entry = runtime_frontend_entry_for_object(&self.runtime(), self.handle())?;
        let exported_lnb_id = {
            let runtime = self.runtime();
            let guard = runtime
                .lock()
                .map_err(|_| status_unknown_error("service runtime lock poisoned"))?;
            guard
                .lnb_for_frontend_id(entry.id.0)
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
        unavailable_after_object_public_api_plan(
            self.plan_method(unsupported_public_api_call(
                AidlObjectKind::Frontend,
                AidlApi::FrontendSetLnb,
                Some(snapshot_value("lnb_id", lnb_id.to_string())),
            )),
            "frontend LNB backend binding is not implemented in this WP",
        )
    }
    fn linkCiCam(&self, ci_cam_id: i32) -> BinderResult<i32> {
        unavailable_after_object_public_api_plan(
            self.plan_method(unsupported_public_api_call(
                AidlObjectKind::Frontend,
                AidlApi::FrontendLinkCiCam,
                Some(snapshot_value("ci_cam_id", ci_cam_id.to_string())),
            )),
            "CI CAM is unsupported",
        )?;
        Err(status_unknown_error(
            "linkCiCam unavailable path unexpectedly returned success",
        ))
    }
    fn unlinkCiCam(&self, ci_cam_id: i32) -> BinderResult<()> {
        unavailable_after_object_public_api_plan(
            self.plan_method(unsupported_public_api_call(
                AidlObjectKind::Frontend,
                AidlApi::FrontendUnlinkCiCam,
                Some(snapshot_value("ci_cam_id", ci_cam_id.to_string())),
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
    fn removeOutputPid(&self, pid: i32) -> BinderResult<()> {
        unavailable_after_object_public_api_plan(
            self.plan_method(unsupported_public_api_call(
                AidlObjectKind::Frontend,
                AidlApi::FrontendRemoveOutputPid,
                Some(snapshot_value("pid", pid.to_string())),
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
            Some(snapshot_fields(
                "FrontendStatusReadiness",
                vec![AidlInputField::new("count", status_types.len().to_string())],
            )),
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
        self.plan_method(AidlMethodCall::DemuxSetFrontendDataSource(snapshot_fields(
            "DemuxSetFrontendDataSource",
            vec![AidlInputField::new("frontend_id", frontend_id.to_string())],
        )))?;
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
        _cb: &Strong<dyn IFilterCallback>,
    ) -> BinderResult<Strong<dyn IFilter>> {
        self.ensure_open()?;
        let snapshot = snapshot_demux_open_filter(filter_type, buffer_size, true);
        self.plan_method(AidlMethodCall::DemuxOpenFilter(snapshot))?;
        let runtime = self.runtime();
        let owner_demux_id =
            runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Demux)?;
        let filter_id = allocate_filter_public_runtime(&runtime, owner_demux_id)?;
        if let Err(error) = runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .register_demux_filter_runtime(
                owner_demux_id,
                filter_id,
                open_kind_for_demux_filter_type(filter_type),
            )
        {
            unregister_child_public_runtime(&runtime, AidlObjectKind::Filter, filter_id)?;
            return Err(status_from_hal_error(error));
        }
        let owner = RuntimeOwnerRelation::Demux {
            demux: self.handle().object_id(),
            generation: self.handle().generation(),
        };
        let child_handle =
            match register_child_aidl_object(&runtime, AidlObjectKind::Filter, filter_id, owner) {
                Ok(handle) => handle,
                Err(status) => {
                    unregister_child_public_runtime(&runtime, AidlObjectKind::Filter, filter_id)?;
                    return Err(status);
                }
            };
        match FilterAidlObject::new(child_handle, runtime.clone()) {
            Ok(object) => Ok(BnFilter::new_binder(object, BinderFeatures::default())),
            Err(_) => {
                rollback_child_aidl_object(&runtime, child_handle)?;
                unregister_child_public_runtime(&runtime, AidlObjectKind::Filter, filter_id)?;
                Err(status_unknown_error("filter object kind mismatch"))
            }
        }
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
                Some(snapshot_strong_handle("IFilter")),
            )),
            "AV sync is not implemented in WP-R03",
        )?;
        Err(status_unknown_error(
            "getAvSyncHwId unavailable path unexpectedly returned success",
        ))
    }
    fn getAvSyncTime(&self, av_sync_hw_id: i32) -> BinderResult<i64> {
        unavailable_after_object_public_api_plan(
            self.plan_method(unsupported_public_api_call(
                AidlObjectKind::Demux,
                AidlApi::DemuxGetAvSyncTime,
                Some(snapshot_value("av_sync_hw_id", av_sync_hw_id.to_string())),
            )),
            "AV sync is not implemented in WP-R03",
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
        _cb: &Strong<dyn IDvrCallback>,
    ) -> BinderResult<Strong<dyn IDvr>> {
        self.ensure_open()?;
        let snapshot = snapshot_demux_open_dvr(dvr_type, buffer_size, true);
        self.plan_method(AidlMethodCall::DemuxOpenDvr(snapshot))?;
        let runtime = self.runtime();
        let owner_demux_id =
            runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Demux)?;
        let dvr_id = allocate_dvr_public_runtime(&runtime, owner_demux_id)?;
        let owner = RuntimeOwnerRelation::Demux {
            demux: self.handle().object_id(),
            generation: self.handle().generation(),
        };
        let child_handle =
            match register_child_aidl_object(&runtime, AidlObjectKind::Dvr, dvr_id, owner) {
                Ok(handle) => handle,
                Err(status) => {
                    unregister_child_public_runtime(&runtime, AidlObjectKind::Dvr, dvr_id)?;
                    return Err(status);
                }
            };
        match DvrAidlObject::new(child_handle, runtime.clone()) {
            Ok(object) => Ok(BnDvr::new_binder(object, BinderFeatures::default())),
            Err(_) => {
                rollback_child_aidl_object(&runtime, child_handle)?;
                unregister_child_public_runtime(&runtime, AidlObjectKind::Dvr, dvr_id)?;
                Err(status_unknown_error("DVR object kind mismatch"))
            }
        }
    }
    fn connectCiCam(&self, ci_cam_id: i32) -> BinderResult<()> {
        unavailable_after_object_public_api_plan(
            self.plan_method(unsupported_public_api_call(
                AidlObjectKind::Demux,
                AidlApi::DemuxConnectCiCam,
                Some(snapshot_value("ci_cam_id", ci_cam_id.to_string())),
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
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::FilterGetQueueDesc),
            "FMQ runtime is not implemented in WP-R03",
        )
    }
    fn close(&self) -> BinderResult<()> {
        self.close_object_after_plan(AidlMethodCall::FilterClose)
    }
    fn configure(&self, _settings: &DemuxFilterSettings) -> BinderResult<()> {
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::FilterConfigure(snapshot_filter_settings(
                _settings,
            ))),
            "filter runtime is not implemented in WP-R03",
        )
    }
    fn configureAvStreamType(&self, _av_stream_type: &AvStreamType) -> BinderResult<()> {
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::FilterConfigureAvStreamType(
                snapshot_av_stream_type(_av_stream_type),
            )),
            "AV stream configuration runtime is not implemented in WP-R03",
        )
    }
    fn configureIpCid(&self, ip_cid: i32) -> BinderResult<()> {
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::FilterConfigure(
                AidlInputSnapshot::single_field("ipCid", "ip_cid", ip_cid.to_string()),
            )),
            "IP CID filtering is outside the TS-only tuner_hal2 profile",
        )
    }
    fn configureMonitorEvent(&self, monitor_event_types: i32) -> BinderResult<()> {
        let plan = self.plan_method(AidlMethodCall::FilterConfigure(
            AidlInputSnapshot::single_field(
                "monitorEventTypes",
                "monitor_event_types",
                monitor_event_types.to_string(),
            ),
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
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::FilterStart),
            "filter runtime is not implemented in WP-R03",
        )
    }
    fn stop(&self) -> BinderResult<()> {
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::FilterStop),
            "filter runtime is not implemented in WP-R03",
        )
    }
    fn flush(&self) -> BinderResult<()> {
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::FilterFlush),
            "filter runtime is not implemented in WP-R03",
        )
    }
    fn getAvSharedHandle(&self, _av_memory: &mut TunerNativeHandle) -> BinderResult<i64> {
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::FilterGetAvSharedHandle),
            "AV shared memory is not implemented in WP-R03",
        )?;
        Err(status_unknown_error(
            "getAvSharedHandle unavailable path unexpectedly returned success",
        ))
    }
    fn getId(&self) -> BinderResult<i32> {
        self.plan_method(AidlMethodCall::FilterGetId)?;
        i32::try_from(self.handle().object_id().0)
            .map_err(|_| status_unknown_error("filter id out of i32 range"))
    }
    fn getId64Bit(&self) -> BinderResult<i64> {
        self.plan_method(AidlMethodCall::FilterGetId64Bit)?;
        Ok(self.handle().object_id().0)
    }
    fn releaseAvHandle(&self, _av_memory: &TunerNativeHandle, av_data_id: i64) -> BinderResult<()> {
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::FilterReleaseAvHandle(snapshot_fields(
                "releaseAvHandle",
                vec![
                    AidlInputField::new("av_data_id", av_data_id.to_string()),
                    AidlInputField::new("native_handle_present", "true"),
                ],
            ))),
            "AV shared memory is not implemented in WP-R03",
        )
    }
    fn setDataSource(&self, filter: &Strong<dyn IFilter>) -> BinderResult<()> {
        self.ensure_open()?;
        self.plan_method(AidlMethodCall::FilterSetDataSource(snapshot_strong_handle(
            "IFilter",
        )))?;
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
        runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .set_filter_data_source_non_null(demux_id, sink_id, source_id)
            .map_err(status_from_hal_error)?;
        Ok(())
    }
    fn setDelayHint(&self, _hint: &FilterDelayHint) -> BinderResult<()> {
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::FilterSetDelayHint(
                snapshot_filter_delay_hint(_hint),
            )),
            "filter delay runtime is not implemented in WP-R03",
        )
    }
}

impl IDvr for DvrAidlObject {
    fn getQueueDesc(&self, _queue: &mut TunerQueueDesc) -> BinderResult<()> {
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::DvrGetQueueDesc),
            "DVR FMQ runtime is not implemented in WP-R03",
        )
    }
    fn configure(&self, _settings: &DvrSettings) -> BinderResult<()> {
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::DvrConfigure(snapshot_dvr_settings(
                _settings,
            ))),
            "DVR runtime is not implemented in WP-R03",
        )
    }
    fn attachFilter(&self, _filter: &Strong<dyn IFilter>) -> BinderResult<()> {
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::DvrAttachFilter(snapshot_strong_handle(
                "IFilter",
            ))),
            "DVR runtime is not implemented in WP-R03",
        )
    }
    fn detachFilter(&self, _filter: &Strong<dyn IFilter>) -> BinderResult<()> {
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::DvrDetachFilter(snapshot_strong_handle(
                "IFilter",
            ))),
            "DVR runtime is not implemented in WP-R03",
        )
    }
    fn start(&self) -> BinderResult<()> {
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::DvrStart),
            "DVR runtime is not implemented in WP-R03",
        )
    }
    fn stop(&self) -> BinderResult<()> {
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::DvrStop),
            "DVR runtime is not implemented in WP-R03",
        )
    }
    fn flush(&self) -> BinderResult<()> {
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::DvrFlush),
            "DVR runtime is not implemented in WP-R03",
        )
    }
    fn close(&self) -> BinderResult<()> {
        self.close_object_after_plan(AidlMethodCall::DvrClose)
    }
    fn setStatusCheckIntervalHint(&self, milliseconds: i64) -> BinderResult<()> {
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::DvrSetStatusCheckIntervalHint(milliseconds)),
            "DVR callback runtime is not implemented in WP-R03",
        )
    }
}

impl IDescrambler for DescramblerAidlObject {
    fn setDemuxSource(&self, demux_id: i32) -> BinderResult<()> {
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::DescramblerSetDemuxSource(demux_id)),
            "descrambler runtime is not implemented in WP-R03",
        )
    }
    fn setKeyToken(&self, key_token: &[u8]) -> BinderResult<()> {
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::DescramblerSetKeyToken(key_token.to_vec())),
            "descrambler key runtime is not implemented in WP-R03",
        )
    }
    fn addPid(
        &self,
        pid: &DemuxPid,
        _optional_upstream_filter: &Strong<dyn IFilter>,
    ) -> BinderResult<()> {
        self.ensure_open()?;
        let pid = ts_pid_from_demux_pid(pid).map_err(status_from_hal_error)?;
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::DescramblerAddPid(pid)),
            "descrambler PID runtime is not implemented in WP-R03",
        )
    }
    fn removePid(
        &self,
        pid: &DemuxPid,
        _optional_upstream_filter: &Strong<dyn IFilter>,
    ) -> BinderResult<()> {
        self.ensure_open()?;
        let pid = ts_pid_from_demux_pid(pid).map_err(status_from_hal_error)?;
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::DescramblerRemovePid(pid)),
            "descrambler PID runtime is not implemented in WP-R03",
        )
    }
    fn close(&self) -> BinderResult<()> {
        self.close_object_after_plan(AidlMethodCall::DescramblerClose)
    }
}

impl ILnb for LnbAidlObject {
    fn setCallback(&self, callback: &Strong<dyn ILnbCallback>) -> BinderResult<()> {
        self.plan_method(AidlMethodCall::LnbSetCallback(snapshot_fields(
            "ILnbCallback",
            vec![AidlInputField::new("strong_present", "true")],
        )))?;
        self.retain_callback(callback)
    }
    fn setVoltage(&self, _voltage: LnbVoltage) -> BinderResult<()> {
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::LnbSetVoltage(snapshot_fields(
                "LnbVoltage",
                vec![AidlInputField::new("voltage", format!("{:?}", _voltage))],
            ))),
            "LNB runtime is not implemented in WP-R03",
        )
    }
    fn setTone(&self, _tone: LnbTone) -> BinderResult<()> {
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::LnbSetTone(snapshot_fields(
                "LnbTone",
                vec![AidlInputField::new("tone", format!("{:?}", _tone))],
            ))),
            "LNB runtime is not implemented in WP-R03",
        )
    }
    fn setSatellitePosition(&self, _position: LnbPosition) -> BinderResult<()> {
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::LnbSetSatellitePosition(snapshot_fields(
                "LnbPosition",
                vec![AidlInputField::new("position", format!("{:?}", _position))],
            ))),
            "LNB runtime is not implemented in WP-R03",
        )
    }
    fn sendDiseqcMessage(&self, diseqc_message: &[u8]) -> BinderResult<()> {
        unavailable_after_method_plan(
            self.plan_method(AidlMethodCall::LnbSendDiseqc(diseqc_message.to_vec())),
            "DiSEqC runtime is not implemented in WP-R03",
        )
    }
    fn close(&self) -> BinderResult<()> {
        self.close_object_after_plan(AidlMethodCall::LnbClose)
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
