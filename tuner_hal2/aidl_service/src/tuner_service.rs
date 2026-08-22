use std::sync::MutexGuard;

use android_hardware_common::aidl::android::hardware::common::NativeHandle::NativeHandle as CommonNativeHandle;
use android_hardware_common_fmq::aidl::android::hardware::common::fmq::GrantorDescriptor::GrantorDescriptor as CommonGrantorDescriptor;
use android_hardware_common_fmq::aidl::android::hardware::common::fmq::MQDescriptor::MQDescriptor as CommonMqDescriptor;
use android_hardware_common_fmq::aidl::android::hardware::common::fmq::SynchronizedReadWrite::SynchronizedReadWrite as CommonSynchronizedReadWrite;
use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::{
    AvStreamType::AvStreamType,
    DemuxCapabilities::DemuxCapabilities,
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
};
use binder::{
    BinderFeatures, Interface, ParcelFileDescriptor, Result as BinderResult, Status, Strong,
};
use maleicacid_tuner_hal2_binder_adapter::{
    aidl_frontend_settings_to_request, aidl_scan_type_to_mode, build_dvr_configure_request,
    build_dvr_open_request, build_filter_av_stream_type_request, build_filter_delay_hint_request,
    build_filter_summary_for_open_type, build_lnb_satellite_position_request,
    build_lnb_tone_request, build_lnb_voltage_request, build_open_filter_request, AidlApi,
    AidlMethodCall, AidlObjectGeneration, AidlObjectId, AidlObjectKind, DvrFilterLinkRequest,
    FilterReleaseAvHandleRequest, FilterSetDataSourceRequest,
};
use maleicacid_tuner_hal2_common::{
    fail_after_cleanup, FrontendBackendKind, FrontendSystem, HalError, HalInternalKind,
    HalInvalidArgumentKind,
};
use maleicacid_tuner_hal2_demux::QueueDescriptorSnapshot;
use maleicacid_tuner_hal2_service_runtime::{
    apply_lnb_satellite_position_object_use_case, apply_lnb_tone_object_use_case,
    apply_lnb_voltage_object_use_case, close_lnb_after_root_open_rollback_use_case,
    lnb_profile_supports_voltage_status, send_lnb_diseqc_object_use_case,
    set_frontend_lnb_object_use_case, FrontendTuneScanTxn,
    ObjectFrontendStatusReadinessValue, ObjectFrontendStatusType, ObjectFrontendStatusValue,
    ObjectQueryRequest, ObjectQueryResponse, RootCommandRequest, RootDemuxCapabilitiesSnapshot,
    RootDemuxInfoSnapshot, RootFrontendInfoSnapshot, RootQueryRequest, RootQueryResponse,
    RuntimeObjectEntry, TunerServiceRuntime,
};

use crate::child_object_open::{
    open_dvr_child_for_owner_object_with_request_builder,
    open_filter_child_for_owner_object_with_request_builder,
};
use crate::demux_object::DemuxAidlObject;
use crate::descrambler_object::DescramblerAidlObject;
use crate::dvr_callback_delivery::{
    deliver_started_dvr_status, is_playback_dvr, start_dvr_status_notifier,
    stop_dvr_status_notifier,
};
use crate::dvr_object::DvrAidlObject;
use crate::error_bridge::{status_from_hal_error, status_unknown_error};
use crate::filter_callback_delivery::AidlFilterEventDispatcher;
use crate::filter_object::FilterAidlObject;
use crate::frontend_callback_delivery::{scan_notifier, tune_notifier};
use crate::frontend_object::FrontendAidlObject;
use crate::lnb_object::LnbAidlObject;
use crate::object_handle::AidlObjectHandle;
use crate::object_runtime::{
    close_object_after_close_preflight, execute_object_query_use_case,
    execute_object_query_use_case_with_aidl_input_conversion, execute_object_runtime_use_case,
    execute_object_runtime_use_case_with_request_builder, execute_shared_object_runtime_use_case,
    execute_shared_object_runtime_use_case_with_request_builder,
    plan_unavailable_object_method_use_case,
};
use crate::service_context::{AidlServiceContext, SharedAidlServiceContext};

mod demux_methods;
mod descrambler_methods;
mod dvr_methods;
mod filter_methods;
mod frontend_methods;
mod lnb_methods;
mod support;

use self::support::public_api_call;

type TunerQueueDesc = CommonMqDescriptor<i8, CommonSynchronizedReadWrite>;
type TunerNativeHandle = CommonNativeHandle;

#[derive(Clone)]
pub struct TunerAidlService {
    context: SharedAidlServiceContext,
}

impl Interface for TunerAidlService {}

fn tuner_hal2_demux_capabilities_from_snapshot(
    snapshot: RootDemuxCapabilitiesSnapshot,
) -> DemuxCapabilities {
    DemuxCapabilities {
        numDemux: snapshot.num_demux,
        numRecord: snapshot.num_record,
        numPlayback: snapshot.num_playback,
        numTsFilter: snapshot.num_ts_filter,
        numSectionFilter: snapshot.num_section_filter,
        numAudioFilter: snapshot.num_audio_filter,
        numVideoFilter: snapshot.num_video_filter,
        numPesFilter: snapshot.num_pes_filter,
        numPcrFilter: snapshot.num_pcr_filter,
        numBytesInSectionFilter: snapshot.num_bytes_in_section_filter,
        filterCaps: snapshot.filter_caps,
        linkCaps: snapshot.link_caps,
        bTimeFilter: snapshot.has_time_filter,
    }
}

fn tuner_hal2_demux_info_from_snapshot(snapshot: RootDemuxInfoSnapshot) -> DemuxInfo {
    DemuxInfo {
        filterTypes: snapshot.filter_types,
    }
}

fn frontend_system_from_type(frontend_type: FrontendType) -> Result<FrontendSystem, HalError> {
    match frontend_type {
        FrontendType::ISDBS => Ok(FrontendSystem::IsdbS),
        FrontendType::ISDBT => Ok(FrontendSystem::IsdbT),
        _ => Err(HalError::invalid_argument(
            HalInvalidArgumentKind::NumericRange,
            "frontend type is not present in the immutable capability snapshot",
        )),
    }
}

fn finish_hal_cleanup_after_primary<T>(
    context: &'static str,
    primary: HalError,
    cleanup: Result<(), HalError>,
) -> BinderResult<T> {
    fail_after_cleanup(context, primary, cleanup).map_err(status_from_hal_error)
}

fn tuner_queue_desc_from_snapshot(snapshot: QueueDescriptorSnapshot) -> TunerQueueDesc {
    let (grantors, fds, ints, quantum, flags) = snapshot.into_parts();
    let mut desc = TunerQueueDesc::default();
    desc.grantors = grantors
        .into_iter()
        .map(|grantor| CommonGrantorDescriptor {
            fdIndex: grantor.fd_index(),
            offset: grantor.offset(),
            extent: grantor.extent(),
        })
        .collect();
    desc.handle = CommonNativeHandle {
        fds: fds.into_iter().map(ParcelFileDescriptor::new).collect(),
        ints,
    };
    desc.quantum = quantum;
    desc.flags = flags;
    desc
}

impl TunerAidlService {
    pub fn new(runtime: TunerServiceRuntime) -> Result<Self, HalError> {
        Self::from_context(AidlServiceContext::shared(runtime))
    }

    pub fn from_context(context: SharedAidlServiceContext) -> Result<Self, HalError> {
        {
            let runtime_handle = context.runtime();
            let mut runtime = runtime_handle.lock().map_err(|_| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "service runtime lock poisoned while installing filter event dispatcher",
                )
            })?;
            runtime.install_filter_event_dispatcher(std::sync::Arc::new(
                AidlFilterEventDispatcher::new(&context),
            ))?;
        }
        Ok(Self { context })
    }

    #[cfg(test)]
    fn new_without_filter_event_dispatcher_for_test(runtime: TunerServiceRuntime) -> Self {
        Self {
            context: AidlServiceContext::shared(runtime),
        }
    }

    pub(crate) fn lock_runtime(&self) -> Result<MutexGuard<'_, TunerServiceRuntime>, Status> {
        self.context.lock_runtime()
    }

    fn handle_from_runtime_entry(entry: RuntimeObjectEntry) -> AidlObjectHandle {
        AidlObjectHandle::new(entry.object_kind(), entry.object_id(), entry.generation())
    }

    fn rollback_root_object_entry_after_aidl_failure_hal(
        &self,
        entry: RuntimeObjectEntry,
        unregister_runtime: bool,
    ) -> Result<(), HalError> {
        let runtime = self.context.runtime();
        let lnb_cleanup_id = {
            let mut guard = runtime.lock().map_err(|_| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "service runtime lock poisoned",
                )
            })?;
            guard
                .root_open_txn()
                .rollback_root_object_entry_after_aidl_failure(entry, unregister_runtime)?
        };
        match lnb_cleanup_id {
            Some(lnb_id) => close_lnb_after_root_open_rollback_use_case(runtime, lnb_id),
            None => Ok(()),
        }
    }

    fn frontend_object_from_entry(
        &self,
        entry: RuntimeObjectEntry,
    ) -> BinderResult<Strong<dyn IFrontend>> {
        if i32::try_from(entry.public_runtime_id().0).is_err() {
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
        match FrontendAidlObject::new(handle, self.context.clone()) {
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
        let public_id = match i32::try_from(entry.public_runtime_id().0) {
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
        match DemuxAidlObject::new(handle, self.context.clone()) {
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
        if i32::try_from(entry.public_runtime_id().0).is_err() {
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
        match DescramblerAidlObject::new(handle, self.context.clone()) {
            Ok(object) => Ok(BnDescrambler::new_binder(object, BinderFeatures::default())),
            Err(_) => finish_hal_cleanup_after_primary(
                "descrambler root object construction rollback failed",
                HalError::internal(HalInternalKind::InvariantViolation, "object kind mismatch"),
                self.rollback_root_object_entry_after_aidl_failure_hal(entry, true),
            ),
        }
    }

    fn lnb_object_from_entry(&self, entry: RuntimeObjectEntry) -> BinderResult<Strong<dyn ILnb>> {
        if i32::try_from(entry.public_runtime_id().0).is_err() {
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
        match LnbAidlObject::new(handle, self.context.clone()) {
            Ok(object) => Ok(BnLnb::new_binder(object, BinderFeatures::default())),
            Err(_) => finish_hal_cleanup_after_primary(
                "LNB root object construction rollback failed",
                HalError::internal(HalInternalKind::InvariantViolation, "object kind mismatch"),
                self.rollback_root_object_entry_after_aidl_failure_hal(entry, false),
            ),
        }
    }
}

fn frontend_type_from_snapshot(snapshot: &RootFrontendInfoSnapshot) -> FrontendType {
    match snapshot.system {
        FrontendSystem::IsdbT => FrontendType::ISDBT,
        FrontendSystem::IsdbS => FrontendType::ISDBS,
        FrontendSystem::IsdbS3 | FrontendSystem::DvbS => FrontendType::ISDBT,
    }
}

fn frontend_status_caps_for_snapshot(
    snapshot: &RootFrontendInfoSnapshot,
) -> Vec<FrontendStatusType> {
    // optional telemetryは保守的に扱う。tune/scan backend runtime接続前は決定的な状態fieldだけをadvertiseする。
    // LNB voltageは、systemがISDB-Sであるだけではなく、frontend exportとexported LNBがprobe/registry由来の同じ固定LNB profileを共有する場合だけadvertiseする。
    let mut caps = Vec::new();
    if snapshot.backend == FrontendBackendKind::LinuxDvb {
        caps.push(FrontendStatusType::DEMOD_LOCK);
    }
    if lnb_profile_supports_voltage_status(snapshot.lnb_profile) {
        caps.push(FrontendStatusType::LNB_VOLTAGE);
    }
    caps
}

fn isdbt_mode_caps() -> i32 {
    FrontendIsdbtMode::AUTO.0
}
fn isdbt_bandwidth_caps() -> i32 {
    FrontendIsdbtBandwidth::AUTO.0 | FrontendIsdbtBandwidth::BANDWIDTH_6MHZ.0
}
fn isdbt_modulation_caps() -> i32 {
    FrontendIsdbtModulation::AUTO.0
}
fn isdbt_coderate_caps() -> i32 {
    FrontendIsdbtCoderate::AUTO.0
}
fn isdbt_guard_interval_caps() -> i32 {
    FrontendIsdbtGuardInterval::AUTO.0
}
fn isdbt_time_interleave_caps() -> i32 {
    FrontendIsdbtTimeInterleaveMode::AUTO.0
}
fn isdbs_modulation_caps() -> i32 {
    FrontendIsdbsModulation::AUTO.0
}
fn isdbs_coderate_caps() -> i32 {
    FrontendIsdbsCoderate::AUTO.0
}

fn frontend_caps_for_snapshot(snapshot: &RootFrontendInfoSnapshot) -> FrontendCapabilities {
    match frontend_type_from_snapshot(snapshot) {
        FrontendType::ISDBT => FrontendCapabilities::IsdbtCaps(FrontendIsdbtCapabilities {
            modeCap: isdbt_mode_caps(),
            bandwidthCap: isdbt_bandwidth_caps(),
            modulationCap: isdbt_modulation_caps(),
            coderateCap: isdbt_coderate_caps(),
            guardIntervalCap: isdbt_guard_interval_caps(),
            timeInterleaveCap: isdbt_time_interleave_caps(),
            isSegmentAuto: snapshot
                .capability
                .isdbt_segment
                .is_some_and(|capability| capability.is_segment_auto),
            isFullSegment: snapshot
                .capability
                .isdbt_segment
                .is_some_and(|capability| capability.is_full_segment),
        }),
        FrontendType::ISDBS => FrontendCapabilities::IsdbsCaps(FrontendIsdbsCapabilities {
            modulationCap: isdbs_modulation_caps(),
            coderateCap: isdbs_coderate_caps(),
        }),
        _ => Default::default(),
    }
}

fn frontend_info_from_snapshot(snapshot: &RootFrontendInfoSnapshot) -> FrontendInfo {
    let scalar = snapshot.capability.scalar;
    FrontendInfo {
        r#type: frontend_type_from_snapshot(snapshot),
        minFrequency: scalar.min_frequency_hz,
        maxFrequency: scalar.max_frequency_hz,
        minSymbolRate: scalar.min_symbol_rate,
        maxSymbolRate: scalar.max_symbol_rate,
        acquireRange: scalar.acquire_range_hz,
        exclusiveGroupId: snapshot.capability.exclusive_group_id,
        statusCaps: frontend_status_caps_for_snapshot(snapshot),
        frontendCaps: frontend_caps_for_snapshot(snapshot),
    }
}

impl ITuner for TunerAidlService {
    fn getFrontendIds(&self) -> BinderResult<Vec<i32>> {
        match self
            .lock_runtime()?
            .execute_root_query(RootQueryRequest::FrontendIds)
            .map_err(status_from_hal_error)?
        {
            RootQueryResponse::FrontendIds(ids) => Ok(ids),
            _ => Err(status_unknown_error(
                "unexpected root query response for getFrontendIds",
            )),
        }
    }

    fn openFrontendById(&self, frontend_id: i32) -> BinderResult<Strong<dyn IFrontend>> {
        let entry = self
            .lock_runtime()?
            .root_open_txn().open_frontend_root_object_for_id(
                frontend_id,
                public_api_call(AidlObjectKind::Tuner, AidlApi::TunerOpenFrontendById, None),
            )
            .map_err(status_from_hal_error)?;
        self.frontend_object_from_entry(entry)
    }

    fn openDemux(&self, demux_id: &mut Vec<i32>) -> BinderResult<Strong<dyn IDemux>> {
        demux_id.clear();
        let entry = self
            .lock_runtime()?
            .root_open_txn().open_demux_root_object(public_api_call(
                AidlObjectKind::Tuner,
                AidlApi::TunerOpenDemux,
                None,
            ))
            .map_err(status_from_hal_error)?;
        let (object, id) = self.demux_object_from_entry(entry, true)?;
        demux_id.push(id);
        Ok(object)
    }

    fn getDemuxCaps(&self) -> BinderResult<DemuxCapabilities> {
        match self
            .lock_runtime()?
            .execute_root_query(RootQueryRequest::DemuxCapabilities)
            .map_err(status_from_hal_error)?
        {
            RootQueryResponse::DemuxCapabilities(snapshot) => {
                Ok(tuner_hal2_demux_capabilities_from_snapshot(snapshot))
            }
            _ => Err(status_unknown_error(
                "unexpected root query response for getDemuxCaps",
            )),
        }
    }

    fn openDescrambler(&self) -> BinderResult<Strong<dyn IDescrambler>> {
        let entry = self
            .lock_runtime()?
            .root_open_txn().open_descrambler_root_object(public_api_call(
                AidlObjectKind::Tuner,
                AidlApi::TunerOpenDescrambler,
                None,
            ))
            .map_err(status_from_hal_error)?;
        self.descrambler_object_from_entry(entry)
    }

    fn getFrontendInfo(&self, frontend_id: i32) -> BinderResult<FrontendInfo> {
        match self
            .lock_runtime()?
            .execute_root_query(RootQueryRequest::FrontendInfo { frontend_id })
            .map_err(status_from_hal_error)?
        {
            RootQueryResponse::FrontendInfo(snapshot) => Ok(frontend_info_from_snapshot(&snapshot)),
            _ => Err(status_unknown_error(
                "unexpected root query response for getFrontendInfo",
            )),
        }
    }

    fn getLnbIds(&self) -> BinderResult<Vec<i32>> {
        match self
            .lock_runtime()?
            .execute_root_query(RootQueryRequest::LnbIds)
            .map_err(status_from_hal_error)?
        {
            RootQueryResponse::LnbIds(ids) => Ok(ids),
            _ => Err(status_unknown_error(
                "unexpected root query response for getLnbIds",
            )),
        }
    }

    fn openLnbById(&self, lnb_id: i32) -> BinderResult<Strong<dyn ILnb>> {
        let entry = self
            .lock_runtime()?
            .root_open_txn().open_lnb_root_object_for_id(
                lnb_id,
                public_api_call(AidlObjectKind::Tuner, AidlApi::TunerOpenLnbById, None),
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
        let (id, entry) = self
            .lock_runtime()?
            .root_open_txn().open_lnb_root_object_by_name(
                lnb_name,
                public_api_call(AidlObjectKind::Tuner, AidlApi::TunerOpenLnbByName, None),
            )
            .map_err(status_from_hal_error)?;
        let object = self.lnb_object_from_entry(entry)?;
        lnb_id.push(id);
        Ok(object)
    }

    fn setLna(&self, _b_enable: bool) -> BinderResult<()> {
        self.lock_runtime()?
            .execute_root_command(RootCommandRequest::SetLna { enabled: _b_enable })
            .map_err(status_from_hal_error)
    }

    fn setMaxNumberOfFrontends(
        &self,
        _frontend_type: FrontendType,
        max_number: i32,
    ) -> BinderResult<()> {
        self.lock_runtime()?
            .execute_root_command(RootCommandRequest::SetMaxNumberOfFrontends {
                frontend_system: frontend_system_from_type(_frontend_type)
                    .map_err(status_from_hal_error)?,
                max_number,
            })
            .map_err(status_from_hal_error)
    }

    fn getMaxNumberOfFrontends(&self, _frontend_type: FrontendType) -> BinderResult<i32> {
        match self
            .lock_runtime()?
            .execute_root_query(RootQueryRequest::MaxNumberOfFrontends {
                frontend_system: frontend_system_from_type(_frontend_type)
                    .map_err(status_from_hal_error)?,
            })
            .map_err(status_from_hal_error)?
        {
            RootQueryResponse::MaxNumberOfFrontends(value) => Ok(value),
            _ => Err(status_unknown_error(
                "unexpected root query response for getMaxNumberOfFrontends",
            )),
        }
    }

    fn isLnaSupported(&self) -> BinderResult<bool> {
        match self
            .lock_runtime()?
            .execute_root_query(RootQueryRequest::LnaSupported)
            .map_err(status_from_hal_error)?
        {
            RootQueryResponse::LnaSupported(supported) => Ok(supported),
            _ => Err(status_unknown_error(
                "unexpected root query response for isLnaSupported",
            )),
        }
    }

    fn getDemuxIds(&self) -> BinderResult<Vec<i32>> {
        match self
            .lock_runtime()?
            .execute_root_query(RootQueryRequest::DemuxIds)
            .map_err(status_from_hal_error)?
        {
            RootQueryResponse::DemuxIds(ids) => Ok(ids),
            _ => Err(status_unknown_error(
                "unexpected root query response for getDemuxIds",
            )),
        }
    }

    fn openDemuxById(&self, demux_id: i32) -> BinderResult<Strong<dyn IDemux>> {
        let entry = self
            .lock_runtime()?
            .root_open_txn().open_demux_root_object_by_id(
                demux_id,
                public_api_call(AidlObjectKind::Tuner, AidlApi::TunerOpenDemuxById, None),
            )
            .map_err(status_from_hal_error)?;
        self.demux_object_from_entry(entry, false)
            .map(|(object, _id)| object)
    }

    fn getDemuxInfo(&self, demux_id: i32) -> BinderResult<DemuxInfo> {
        match self
            .lock_runtime()?
            .execute_root_query(RootQueryRequest::DemuxInfo { demux_id })
            .map_err(status_from_hal_error)?
        {
            RootQueryResponse::DemuxInfo(snapshot) => {
                Ok(tuner_hal2_demux_info_from_snapshot(snapshot))
            }
            _ => Err(status_unknown_error(
                "unexpected root query response for getDemuxInfo",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use maleicacid_tuner_hal2_binder_adapter::{DvrOpenKind, OpenDvrRequest};
    use maleicacid_tuner_hal2_service_runtime::{
        ObjectMethodUseCase, RuntimeOwnerRelation,
    };

    #[test]
    fn configure_ip_cid_returns_unavailable_for_any_value() {
        let service = TunerAidlService::new_without_filter_event_dispatcher_for_test(
            TunerServiceRuntime::new(),
        );
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
        let filter = FilterAidlObject::new(handle, service.context.clone()).unwrap();
        assert!(filter.configureIpCid(-1).is_err());
    }

    #[test]
    fn configure_monitor_event_zero_succeeds_nonzero_unavailable() {
        let service = TunerAidlService::new_without_filter_event_dispatcher_for_test(
            TunerServiceRuntime::new(),
        );
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
        let filter = FilterAidlObject::new(handle, service.context.clone()).unwrap();
        assert!(filter.configureMonitorEvent(0).is_ok());
        assert!(filter.configureMonitorEvent(1).is_err());
    }

    #[test]
    fn dvr_close_rejects_second_close_and_closed_object_rejects_start() {
        let service = TunerAidlService::new_without_filter_event_dispatcher_for_test(
            TunerServiceRuntime::new(),
        );
        let runtime = service.context.runtime();
        let demux_entry = {
            let mut guard = runtime.lock().unwrap();
            guard
                .root_open_txn().open_demux_root_object(public_api_call(
                    AidlObjectKind::Tuner,
                    AidlApi::TunerOpenDemux,
                    None,
                ))
                .unwrap()
        };
        let dvr_open = ObjectMethodUseCase::execute_after_live(
            &runtime,
            demux_entry.object_id(),
            demux_entry.generation(),
            AidlObjectKind::Demux,
            || -> Result<_, maleicacid_tuner_hal2_common::HalError> {
                let request = OpenDvrRequest {
                    kind: DvrOpenKind::Playback,
                    buffer_size: 188,
                };
                Ok((AidlMethodCall::DemuxOpenDvr(request.clone()), request))
            },
            |runtime, dispatch, request| {
                runtime.child_open_txn().open_dvr_child_runtime_for_demux_object(
                    demux_entry.object_id(),
                    demux_entry.generation(),
                    request,
                    dispatch,
                )
            },
        )
        .unwrap();
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Dvr,
            dvr_open.runtime_entry.object_id(),
            dvr_open.runtime_entry.generation(),
        );
        let dvr = DvrAidlObject::new(handle, service.context.clone()).unwrap();

        assert!(dvr.close().is_ok());
        assert!(dvr.close().is_err());
        assert!(dvr.start().is_err());
    }

    #[test]
    fn px4_does_not_advertise_current_demod_lock_readback() {
        let capability = maleicacid_tuner_hal2_service_runtime::FrontendCapabilitySnapshot {
            scalar: maleicacid_tuner_hal2_service_runtime::FrontendScalarCapability {
                min_frequency_hz: 1,
                max_frequency_hz: 2,
                min_symbol_rate: 0,
                max_symbol_rate: 0,
                acquire_range_hz: 0,
            },
            exclusive_group_id: 1,
            isdbt_segment: None,
        };
        let px4 = RootFrontendInfoSnapshot {
            id: 1,
            backend: FrontendBackendKind::Px4CharDevice,
            system: FrontendSystem::IsdbT,
            lnb_profile: None,
            capability,
        };
        let dvb = RootFrontendInfoSnapshot {
            backend: FrontendBackendKind::LinuxDvb,
            ..px4
        };

        assert!(!frontend_status_caps_for_snapshot(&px4)
            .contains(&FrontendStatusType::DEMOD_LOCK));
        assert!(frontend_status_caps_for_snapshot(&dvb)
            .contains(&FrontendStatusType::DEMOD_LOCK));
    }
}
