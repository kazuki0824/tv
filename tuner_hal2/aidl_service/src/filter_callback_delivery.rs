use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, Weak};

use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::{
    DemuxFilterEvent::DemuxFilterEvent, DemuxFilterMediaEvent::DemuxFilterMediaEvent,
    DemuxFilterPesEvent::DemuxFilterPesEvent, DemuxFilterScIndexMask::DemuxFilterScIndexMask,
    DemuxFilterSectionEvent::DemuxFilterSectionEvent,
    DemuxFilterStatus::DemuxFilterStatus,
    DemuxFilterTsRecordEvent::DemuxFilterTsRecordEvent, DemuxPid::DemuxPid,
};
use android_hardware_common::aidl::android::hardware::common::NativeHandle::NativeHandle;
use binder::ParcelFileDescriptor;
use maleicacid_tuner_hal2_binder_adapter::AidlObjectKind;
use maleicacid_tuner_hal2_common::{FirstErrorCollector, HalError, HalInternalKind};
use maleicacid_tuner_hal2_demux::{
    FilterStatusEvent, TsRecordEventData, RECORD_SC_TYPE_SC, RECORD_SC_TYPE_SC_AVC,
    RECORD_SC_TYPE_SC_HEVC, RECORD_SC_TYPE_SC_VVC,
};
use maleicacid_tuner_hal2_service_runtime::{
    filter_delivery_wake_sequence, notify_filter_delivery_change, wait_filter_delivery_change,
    CallbackDeliveryFailurePhase, CallbackDeliveryFailureReport,
    FilterCallbackDeliveryDiagnosticPhase, FilterCallbackDeliveryDiagnosticRecord,
    FilterEventDelivery, FilterEventDeliverySnapshot, FilterEventDispatcher, TunerServiceRuntime,
    WorkerRuntime,
};

use crate::object_handle::AidlObjectHandle;
use crate::service_context::{AidlServiceContext, SharedAidlServiceContext};

pub struct AidlFilterEventDispatcher {
    context: Weak<AidlServiceContext>,
    delay_worker: Option<WorkerRuntime<()>>,
}

enum AidlFilterCallbackDelivery {
    Event(DemuxFilterEvent),
    Status(DemuxFilterStatus),
}

impl AidlFilterCallbackDelivery {
    const fn operation_name(&self) -> &'static str {
        match self {
            Self::Event(_) => "IFilterCallback.onFilterEvent",
            Self::Status(_) => "IFilterCallback.onFilterStatus",
        }
    }
}

impl AidlFilterEventDispatcher {
    pub fn new(context: &SharedAidlServiceContext) -> Result<Self, HalError> {
        let worker_context = Arc::downgrade(context);
        let delay_worker = WorkerRuntime::spawn(
            "maleicacid-filter-delay-delivery".to_string(),
            0,
            1,
            move |stop| run_filter_delay_delivery(worker_context, stop),
            notify_filter_delivery_change,
        )
        .map_err(|error| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                format!("filter delay delivery worker spawn failed: {error}"),
            )
        })?;
        Ok(Self {
            context: Arc::downgrade(context),
            delay_worker: Some(delay_worker),
        })
    }

    fn without_worker(context: &SharedAidlServiceContext) -> Self {
        Self {
            context: Arc::downgrade(context),
            delay_worker: None,
        }
    }
}

impl Drop for AidlFilterEventDispatcher {
    fn drop(&mut self) {
        if let Some(worker) = self.delay_worker.as_ref() {
            worker.request_stop_and_wake();
            notify_filter_delivery_change();
        }
    }
}

pub(crate) fn dispatch_filter_event_snapshots(
    context: &SharedAidlServiceContext,
    snapshots: Vec<FilterEventDeliverySnapshot>,
) -> Result<(), HalError> {
    if snapshots.is_empty() {
        return Ok(());
    }
    let runtime = context.runtime();
    AidlFilterEventDispatcher::without_worker(context).dispatch(&runtime, snapshots)
}

fn run_filter_delay_delivery(
    weak_context: Weak<AidlServiceContext>,
    stop: Arc<std::sync::atomic::AtomicBool>,
) -> Result<(), HalError> {
    loop {
        if stop.load(std::sync::atomic::Ordering::Acquire) {
            return Ok(());
        }
        let observed = filter_delivery_wake_sequence();
        let Some(context) = weak_context.upgrade() else {
            return Ok(());
        };
        let runtime = context.runtime();
        let (snapshots, deadline) = {
            let mut guard = runtime.lock().map_err(|_| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "service runtime lock poisoned while polling delayed filter events",
                )
            })?;
            guard.poll_filter_delay_delivery()?
        };
        if !snapshots.is_empty() {
            let _recorded_failure = dispatch_filter_event_snapshots(&context, snapshots);
            continue;
        }
        drop(runtime);
        drop(context);
        if stop.load(std::sync::atomic::Ordering::Acquire) {
            return Ok(());
        }
        let _ = wait_filter_delivery_change(observed, deadline);
    }
}

fn aidl_media_timestamp_90khz(
    is_present: bool,
    value: Option<u64>,
    field_name: &'static str,
) -> Result<i64, HalError> {
    if is_present && value.is_none() {
        return Err(HalError::internal(
            HalInternalKind::InvariantViolation,
            format!("filter media event {field_name} presence has no value"),
        ));
    }
    let Some(value) = value else {
        return Ok(0);
    };
    if value >= (1_u64 << 33) {
        return Err(HalError::internal(
            HalInternalKind::InvariantViolation,
            format!("filter media event {field_name} exceeds the 33-bit MPEG timestamp domain"),
        ));
    }
    i64::try_from(value).map_err(|_| {
        HalError::internal(
            HalInternalKind::InvariantViolation,
            format!("filter media event {field_name} does not fit i64"),
        )
    })
}

fn event_from_snapshot(
    snapshot: FilterEventDeliverySnapshot,
) -> Result<AidlFilterCallbackDelivery, HalError> {
    match snapshot.event {
        FilterEventDelivery::StartId(start_id) => Ok(AidlFilterCallbackDelivery::Event(
            DemuxFilterEvent::StartId(start_id),
        )),
        FilterEventDelivery::Status(status) => Ok(AidlFilterCallbackDelivery::Status(
            match status {
                FilterStatusEvent::DataReady => DemuxFilterStatus::DATA_READY,
                FilterStatusEvent::LowWater => DemuxFilterStatus::LOW_WATER,
                FilterStatusEvent::HighWater => DemuxFilterStatus::HIGH_WATER,
                FilterStatusEvent::Overflow => DemuxFilterStatus::OVERFLOW,
            },
        )),
        FilterEventDelivery::Media(event) => {
            let data_length = i64::try_from(event.data_length).map_err(|_| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "filter media event length does not fit i64",
                )
            })?;
            let offset = i64::try_from(event.offset).map_err(|_| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "filter media event offset does not fit i64",
                )
            })?;
            let pts = aidl_media_timestamp_90khz(
                event.metadata.is_pts_present,
                event.metadata.pts_90khz,
                "PTS",
            )?;
            let dts = aidl_media_timestamp_90khz(
                event.metadata.is_dts_present,
                event.metadata.dts_90khz,
                "DTS",
            )?;
            let av_memory = match event.event_local_file {
                Some(file) => NativeHandle {
                    fds: vec![ParcelFileDescriptor::new(file.try_clone().map_err(|_| {
                        HalError::internal(
                            HalInternalKind::InvariantViolation,
                            "event-local AV handle duplication failed",
                        )
                    })?)],
                    ints: vec![0],
                },
                None => Default::default(),
            };
            Ok(AidlFilterCallbackDelivery::Event(DemuxFilterEvent::Media(DemuxFilterMediaEvent {
                streamId: i32::from(event.metadata.stream_id),
                isPtsPresent: event.metadata.is_pts_present,
                pts,
                isDtsPresent: event.metadata.is_dts_present,
                dts,
                dataLength: data_length,
                offset,
                avDataId: event.data_id.0,
                avMemory: av_memory,
                isPesPrivateData: event.metadata.is_pes_private_data,
                ..Default::default()
            })))
        }
        FilterEventDelivery::Section { data_length } => {
            let data_length = i64::try_from(data_length).map_err(|_| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "filter section event length does not fit i64",
                )
            })?;
            Ok(AidlFilterCallbackDelivery::Event(DemuxFilterEvent::Section(DemuxFilterSectionEvent {
                dataLength: data_length,
                ..Default::default()
            })))
        }
        FilterEventDelivery::Pes {
            stream_id,
            data_length,
        } => {
            let data_length = i32::try_from(data_length).map_err(|_| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "filter PES event length does not fit i32",
                )
            })?;
            Ok(AidlFilterCallbackDelivery::Event(DemuxFilterEvent::Pes(DemuxFilterPesEvent {
                streamId: stream_id,
                dataLength: data_length,
                ..Default::default()
            })))
        }
        FilterEventDelivery::RecordIndex(event) => Ok(AidlFilterCallbackDelivery::Event(
            DemuxFilterEvent::TsRecord(DemuxFilterTsRecordEvent {
                pid: DemuxPid::TPid(event.pid.to_i32_for_aidl_boundary()),
                tsIndexMask: event.ts_index_mask,
                scIndexMask: aidl_sc_index_mask_from_record_event(event),
                byteNumber: event.byte_number,
                pts: event.pts,
                firstMbInSlice: event.first_mb_in_slice,
            }),
        )),
    }
}

fn aidl_sc_index_mask_from_record_event(event: TsRecordEventData) -> DemuxFilterScIndexMask {
    match event.sc_index_type {
        RECORD_SC_TYPE_SC => DemuxFilterScIndexMask::ScIndex(event.sc_index_mask_bits),
        RECORD_SC_TYPE_SC_AVC => DemuxFilterScIndexMask::ScAvc(event.sc_index_mask_bits),
        RECORD_SC_TYPE_SC_HEVC => DemuxFilterScIndexMask::ScHevc(event.sc_index_mask_bits),
        RECORD_SC_TYPE_SC_VVC => DemuxFilterScIndexMask::ScVvc(event.sc_index_mask_bits),
        _ => DemuxFilterScIndexMask::ScIndex(0),
    }
}

fn filter_callback_diagnostic_phase(
    phase: CallbackDeliveryFailurePhase,
) -> FilterCallbackDeliveryDiagnosticPhase {
    match phase {
        CallbackDeliveryFailurePhase::CallbackArtifactLookup
        | CallbackDeliveryFailurePhase::RuntimePolicySkip
        | CallbackDeliveryFailurePhase::NotifierCleanup
        | CallbackDeliveryFailurePhase::NotifierPreflight => {
            FilterCallbackDeliveryDiagnosticPhase::CallbackRegistryAccounting
        }
        CallbackDeliveryFailurePhase::EventConversion
        | CallbackDeliveryFailurePhase::BinderDelivery
        | CallbackDeliveryFailurePhase::ScanEndDelivery
        | CallbackDeliveryFailurePhase::PostCommitNotification
        | CallbackDeliveryFailurePhase::NotifierTerminal => {
            FilterCallbackDeliveryDiagnosticPhase::EventDelivery
        }
    }
}

fn finish_filter_callback_delivery_failure(
    context: &SharedAidlServiceContext,
    runtime: &Arc<Mutex<TunerServiceRuntime>>,
    handle: AidlObjectHandle,
    phase: CallbackDeliveryFailurePhase,
    primary: HalError,
) -> Result<(), HalError> {
    match runtime.lock() {
        Ok(mut runtime) => runtime.finish_callback_delivery_failure_use_case(
            CallbackDeliveryFailureReport::filter(
                handle.object_id(),
                handle.generation(),
                phase,
                primary,
            ),
        ),
        Err(_) => {
            let record = FilterCallbackDeliveryDiagnosticRecord::new(
                filter_callback_diagnostic_phase(phase),
                handle.object_id(),
                handle.generation(),
                primary.clone(),
            );
            match context.record_filter_callback_delivery_failure_fallback(record) {
                Ok(()) => Err(primary),
                Err(record_error) => Err(
                    maleicacid_tuner_hal2_common::compose_primary_cleanup_failure(
                        "filter callback delivery fallback diagnostic record failed",
                        primary,
                        record_error,
                    ),
                ),
            }
        }
    }
}

impl FilterEventDispatcher for AidlFilterEventDispatcher {
    fn dispatch(
        &self,
        runtime: &Arc<Mutex<TunerServiceRuntime>>,
        snapshots: Vec<FilterEventDeliverySnapshot>,
    ) -> Result<(), HalError> {
        let mut failures = FirstErrorCollector::new();
        let mut start_id_blocked = BTreeSet::new();
        for snapshot in snapshots {
            let delivery_key = (snapshot.object_id.0, snapshot.generation.0);
            if start_id_blocked.contains(&delivery_key) {
                continue;
            }
            let pending_start_id = match &snapshot.event {
                FilterEventDelivery::StartId(start_id) => Some(*start_id),
                _ => None,
            };
            let filter_id = snapshot.filter_id;
            let handle = AidlObjectHandle::new(
                AidlObjectKind::Filter,
                snapshot.object_id,
                snapshot.generation,
            );
            let context = match self.context.upgrade() {
                Some(context) => context,
                None => {
                    failures.push_error(HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "AIDL service context is not available for filter callback delivery",
                    ));
                    if pending_start_id.is_some() {
                        start_id_blocked.insert(delivery_key);
                    }
                    continue;
                }
            };
            let callback = match context.filter_callback_for_owner(handle) {
                Ok(Some(callback)) => callback,
                Ok(None) => {
                    let primary = HalError::callback_failed(
                        "IFilterCallback.onFilterEvent",
                        "filter callback artifact is not registered",
                    );
                    failures.push_result(finish_filter_callback_delivery_failure(
                        &context,
                        runtime,
                        handle,
                        CallbackDeliveryFailurePhase::CallbackArtifactLookup,
                        primary,
                    ));
                    if pending_start_id.is_some() {
                        start_id_blocked.insert(delivery_key);
                    }
                    continue;
                }
                Err(error) => {
                    let primary = error.into_hal_error("IFilterCallback.onFilterEvent");
                    failures.push_result(finish_filter_callback_delivery_failure(
                        &context,
                        runtime,
                        handle,
                        CallbackDeliveryFailurePhase::CallbackArtifactLookup,
                        primary,
                    ));
                    if pending_start_id.is_some() {
                        start_id_blocked.insert(delivery_key);
                    }
                    continue;
                }
            };
            let delivery = match event_from_snapshot(snapshot) {
                Ok(delivery) => delivery,
                Err(primary) => {
                    failures.push_result(finish_filter_callback_delivery_failure(
                        &context,
                        runtime,
                        handle,
                        CallbackDeliveryFailurePhase::EventConversion,
                        primary,
                    ));
                    if pending_start_id.is_some() {
                        start_id_blocked.insert(delivery_key);
                    }
                    continue;
                }
            };
            let operation = delivery.operation_name();
            let delivery_result = match delivery {
                AidlFilterCallbackDelivery::Event(event) => callback.onFilterEvent(&[event]),
                AidlFilterCallbackDelivery::Status(status) => callback.onFilterStatus(status),
            };
            if let Err(error) = delivery_result {
                let primary = HalError::callback_failed(
                    operation,
                    format!("binder failure: {error:?}"),
                );
                failures.push_result(finish_filter_callback_delivery_failure(
                    &context,
                    runtime,
                    handle,
                    CallbackDeliveryFailurePhase::BinderDelivery,
                    primary,
                ));
                if pending_start_id.is_some() {
                    start_id_blocked.insert(delivery_key);
                }
                continue;
            }
            if let Some(start_id) = pending_start_id {
                let commit_result = runtime
                    .lock()
                    .map_err(|_| {
                        HalError::internal(
                            HalInternalKind::InvariantViolation,
                            "service runtime lock poisoned while committing filter startId delivery",
                        )
                    })
                    .and_then(|mut runtime| {
                        runtime.commit_filter_start_id_delivery(
                            handle.object_id(),
                            handle.generation(),
                            filter_id,
                            start_id,
                        )
                    });
                if let Err(error) = commit_result {
                    failures.push_error(error);
                    start_id_blocked.insert(delivery_key);
                }
            }
        }
        failures.into_result()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use maleicacid_tuner_hal2_binder_adapter::{AidlObjectGeneration, AidlObjectId};
    use maleicacid_tuner_hal2_demux::{
        AvDataId, AvMediaEventDescriptor, AvMediaEventMetadata, AvSlotId,
    };

    fn snapshot(event: FilterEventDelivery) -> FilterEventDeliverySnapshot {
        FilterEventDeliverySnapshot {
            object_id: AidlObjectId(1),
            generation: AidlObjectGeneration(2),
            filter_id: 3,
            event,
        }
    }

    #[test]
    fn media_section_and_pes_snapshots_keep_delivery_lengths() {
        let media = event_from_snapshot(snapshot(FilterEventDelivery::Media(
            AvMediaEventDescriptor {
                data_id: AvDataId(7),
                slot_id: AvSlotId(0),
                offset: 12,
                data_length: 188,
                metadata: AvMediaEventMetadata::default(),
                event_local_file: None,
            },
        )))
        .unwrap();
        assert!(matches!(
            media,
            AidlFilterCallbackDelivery::Event(DemuxFilterEvent::Media(DemuxFilterMediaEvent {
                avDataId: 7,
                offset: 12,
                dataLength: 188,
                ..
            }))
        ));

        let section =
            event_from_snapshot(snapshot(FilterEventDelivery::Section { data_length: 64 }))
                .unwrap();
        assert!(matches!(
            section,
            AidlFilterCallbackDelivery::Event(DemuxFilterEvent::Section(
                DemuxFilterSectionEvent { dataLength: 64, .. }
            ))
        ));

        let pes = event_from_snapshot(snapshot(FilterEventDelivery::Pes {
            stream_id: 256,
            data_length: 1024,
        }))
        .unwrap();
        assert!(matches!(
            pes,
            AidlFilterCallbackDelivery::Event(DemuxFilterEvent::Pes(DemuxFilterPesEvent {
                streamId: 256,
                dataLength: 1024,
                ..
            }))
        ));
    }

    #[test]
    fn start_id_snapshot_projects_to_the_start_id_union_variant() {
        let delivery = event_from_snapshot(snapshot(FilterEventDelivery::StartId(7))).unwrap();
        assert!(matches!(
            delivery,
            AidlFilterCallbackDelivery::Event(DemuxFilterEvent::StartId(7))
        ));
    }

    fn projected_media_event(
        metadata: AvMediaEventMetadata,
    ) -> Result<DemuxFilterMediaEvent, HalError> {
        match event_from_snapshot(snapshot(FilterEventDelivery::Media(
            AvMediaEventDescriptor {
                data_id: AvDataId(7),
                slot_id: AvSlotId(0),
                offset: 12,
                data_length: 188,
                metadata,
                event_local_file: None,
            },
        )))? {
            AidlFilterCallbackDelivery::Event(DemuxFilterEvent::Media(event)) => Ok(event),
            _ => panic!("media snapshot must project to a media event"),
        }
    }

    #[test]
    fn media_event_preserves_explicit_pes_pts() {
        let event = projected_media_event(AvMediaEventMetadata::from_pes(
            0xe0,
            Some(90_001),
            None,
            false,
        ))
        .unwrap();

        assert_eq!(event.streamId, 0xe0);
        assert!(event.isPtsPresent);
        assert_eq!(event.pts, 90_001);
        assert!(!event.isDtsPresent);
        assert_eq!(event.dts, 0);
    }

    #[test]
    fn media_event_preserves_explicit_pes_pts_and_dts() {
        let event = projected_media_event(AvMediaEventMetadata::from_pes(
            0xe0,
            Some(180_001),
            Some(90_001),
            false,
        ))
        .unwrap();

        assert_eq!(event.streamId, 0xe0);
        assert!(event.isPtsPresent);
        assert_eq!(event.pts, 180_001);
        assert!(event.isDtsPresent);
        assert_eq!(event.dts, 90_001);
    }

    #[test]
    fn media_event_preserves_legal_pes_timestamp_absence() {
        let event =
            projected_media_event(AvMediaEventMetadata::from_pes(0xc0, None, None, false))
                .unwrap();

        assert_eq!(event.streamId, 0xc0);
        assert!(!event.isPtsPresent);
        assert_eq!(event.pts, 0);
        assert!(!event.isDtsPresent);
        assert_eq!(event.dts, 0);
    }

    #[test]
    fn media_event_keeps_non_header_authoritative_pts_provenance() {
        let event = projected_media_event(AvMediaEventMetadata {
            stream_id: 0xc0,
            is_pts_present: false,
            pts_90khz: Some(270_001),
            is_dts_present: false,
            dts_90khz: None,
            is_pes_private_data: false,
        })
        .unwrap();

        assert_eq!(event.streamId, 0xc0);
        assert!(!event.isPtsPresent);
        assert_eq!(event.pts, 270_001);
    }

    #[test]
    fn media_event_preserves_authoritative_pes_private_data_presence() {
        let event =
            projected_media_event(AvMediaEventMetadata::from_pes(0xe0, None, None, true)).unwrap();

        assert!(event.isPesPrivateData);
    }

    #[test]
    fn filter_status_snapshots_map_to_aidl_status_callbacks() {
        for (status, expected) in [
            (FilterStatusEvent::DataReady, DemuxFilterStatus::DATA_READY),
            (FilterStatusEvent::LowWater, DemuxFilterStatus::LOW_WATER),
            (FilterStatusEvent::HighWater, DemuxFilterStatus::HIGH_WATER),
            (FilterStatusEvent::Overflow, DemuxFilterStatus::OVERFLOW),
        ] {
            let delivery = event_from_snapshot(snapshot(FilterEventDelivery::Status(status)))
                .unwrap();
            assert!(matches!(
                delivery,
                AidlFilterCallbackDelivery::Status(actual) if actual == expected
            ));
        }
    }
}
