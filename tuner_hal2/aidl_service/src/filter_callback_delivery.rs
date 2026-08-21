use std::sync::{Arc, Mutex, Weak};

use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::{
    DemuxFilterEvent::DemuxFilterEvent, DemuxFilterMediaEvent::DemuxFilterMediaEvent,
    DemuxFilterPesEvent::DemuxFilterPesEvent, DemuxFilterScIndexMask::DemuxFilterScIndexMask,
    DemuxFilterSectionEvent::DemuxFilterSectionEvent,
    DemuxFilterTsRecordEvent::DemuxFilterTsRecordEvent, DemuxPid::DemuxPid,
};
use android_hardware_common::aidl::android::hardware::common::NativeHandle::NativeHandle;
use binder::ParcelFileDescriptor;
use maleicacid_tuner_hal2_binder_adapter::AidlObjectKind;
use maleicacid_tuner_hal2_common::{FirstErrorCollector, HalError, HalInternalKind};
use maleicacid_tuner_hal2_demux::{
    TsRecordEventData, RECORD_SC_TYPE_SC, RECORD_SC_TYPE_SC_AVC, RECORD_SC_TYPE_SC_HEVC,
    RECORD_SC_TYPE_SC_VVC,
};
use maleicacid_tuner_hal2_service_runtime::{
    CallbackDeliveryFailurePhase, CallbackDeliveryFailureReport,
    FilterCallbackDeliveryDiagnosticPhase, FilterCallbackDeliveryDiagnosticRecord,
    FilterEventDelivery, FilterEventDeliverySnapshot, FilterEventDispatcher, TunerServiceRuntime,
};

use crate::object_handle::AidlObjectHandle;
use crate::service_context::{AidlServiceContext, SharedAidlServiceContext};

pub struct AidlFilterEventDispatcher {
    context: Weak<AidlServiceContext>,
}

impl AidlFilterEventDispatcher {
    pub fn new(context: &SharedAidlServiceContext) -> Self {
        Self {
            context: Arc::downgrade(context),
        }
    }
}

fn event_from_snapshot(
    snapshot: FilterEventDeliverySnapshot,
) -> Result<DemuxFilterEvent, HalError> {
    match snapshot.event {
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
            Ok(DemuxFilterEvent::Media(DemuxFilterMediaEvent {
                dataLength: data_length,
                offset,
                avDataId: event.data_id.0,
                avMemory: av_memory,
                ..Default::default()
            }))
        }
        FilterEventDelivery::Section { data_length } => {
            let data_length = i64::try_from(data_length).map_err(|_| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "filter section event length does not fit i64",
                )
            })?;
            Ok(DemuxFilterEvent::Section(DemuxFilterSectionEvent {
                dataLength: data_length,
                ..Default::default()
            }))
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
            Ok(DemuxFilterEvent::Pes(DemuxFilterPesEvent {
                streamId: stream_id,
                dataLength: data_length,
                ..Default::default()
            }))
        }
        FilterEventDelivery::RecordIndex(event) => {
            Ok(DemuxFilterEvent::TsRecord(DemuxFilterTsRecordEvent {
                pid: DemuxPid::TPid(event.pid.to_i32_for_aidl_boundary()),
                tsIndexMask: event.ts_index_mask,
                scIndexMask: aidl_sc_index_mask_from_record_event(event),
                byteNumber: event.byte_number,
                pts: event.pts,
                firstMbInSlice: event.first_mb_in_slice,
            }))
        }
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
        for snapshot in snapshots {
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
                    continue;
                }
            };
            let event = match event_from_snapshot(snapshot) {
                Ok(event) => event,
                Err(primary) => {
                    failures.push_result(finish_filter_callback_delivery_failure(
                        &context,
                        runtime,
                        handle,
                        CallbackDeliveryFailurePhase::EventConversion,
                        primary,
                    ));
                    continue;
                }
            };
            if let Err(error) = callback.onFilterEvent(&[event]) {
                let primary = HalError::callback_failed(
                    "IFilterCallback.onFilterEvent",
                    format!("binder failure: {error:?}"),
                );
                failures.push_result(finish_filter_callback_delivery_failure(
                    &context,
                    runtime,
                    handle,
                    CallbackDeliveryFailurePhase::BinderDelivery,
                    primary,
                ));
            }
        }
        failures.into_result()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use maleicacid_tuner_hal2_binder_adapter::{AidlObjectGeneration, AidlObjectId};
    use maleicacid_tuner_hal2_demux::AvMediaEventDescriptor;
    use maleicacid_tuner_hal2_demux::{AvDataId, AvSlotId};

    fn snapshot(event: FilterEventDelivery) -> FilterEventDeliverySnapshot {
        FilterEventDeliverySnapshot {
            object_id: AidlObjectId(1),
            generation: AidlObjectGeneration(2),
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
                event_local_file: None,
            },
        )))
        .unwrap();
        assert!(matches!(
            media,
            DemuxFilterEvent::Media(DemuxFilterMediaEvent {
                avDataId: 7,
                offset: 12,
                dataLength: 188,
                ..
            })
        ));

        let section =
            event_from_snapshot(snapshot(FilterEventDelivery::Section { data_length: 64 }))
                .unwrap();
        assert!(matches!(
            section,
            DemuxFilterEvent::Section(DemuxFilterSectionEvent { dataLength: 64, .. })
        ));

        let pes = event_from_snapshot(snapshot(FilterEventDelivery::Pes {
            stream_id: 256,
            data_length: 1024,
        }))
        .unwrap();
        assert!(matches!(
            pes,
            DemuxFilterEvent::Pes(DemuxFilterPesEvent {
                streamId: 256,
                dataLength: 1024,
                ..
            })
        ));
    }
}
