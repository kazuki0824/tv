use std::sync::{Arc, Mutex, Weak};

use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::{
    DemuxFilterEvent::DemuxFilterEvent, DemuxFilterMediaEvent::DemuxFilterMediaEvent,
    DemuxFilterPesEvent::DemuxFilterPesEvent, DemuxFilterSectionEvent::DemuxFilterSectionEvent,
};
use maleicacid_tuner_hal2_binder_adapter::AidlObjectKind;
use maleicacid_tuner_hal2_common::{HalError, HalInternalKind};
use maleicacid_tuner_hal2_service_runtime::{
    CallbackDeliveryFailurePhase, CallbackDeliveryFailureReport, FilterEventDelivery,
    FilterEventDeliverySnapshot, FilterEventDispatcher, TunerServiceRuntime,
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
            Ok(DemuxFilterEvent::Media(DemuxFilterMediaEvent {
                dataLength: data_length,
                offset,
                avDataId: event.data_id.0,
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
    }
}

fn finish_filter_callback_delivery_failure(
    runtime: &Arc<Mutex<TunerServiceRuntime>>,
    handle: AidlObjectHandle,
    phase: CallbackDeliveryFailurePhase,
    primary: HalError,
) -> Result<(), HalError> {
    let mut runtime = runtime.lock().map_err(|_| {
        HalError::internal(
            HalInternalKind::InvariantViolation,
            "service runtime lock poisoned while finishing filter callback delivery failure",
        )
    })?;
    runtime.finish_callback_delivery_failure_use_case(CallbackDeliveryFailureReport::filter(
        handle.object_id(),
        handle.generation(),
        phase,
        primary,
    ))
}

impl FilterEventDispatcher for AidlFilterEventDispatcher {
    fn dispatch(
        &self,
        runtime: &Arc<Mutex<TunerServiceRuntime>>,
        snapshots: Vec<FilterEventDeliverySnapshot>,
    ) -> Result<(), HalError> {
        for snapshot in snapshots {
            let handle = AidlObjectHandle::new(
                AidlObjectKind::Filter,
                snapshot.object_id,
                snapshot.generation,
            );
            let context = self.context.upgrade().ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "AIDL service context is not available for filter callback delivery",
                )
            })?;
            let callback = match context.filter_callback_for_owner(handle) {
                Ok(Some(callback)) => callback,
                Ok(None) => {
                    let primary = HalError::callback_failed(
                        "IFilterCallback.onFilterEvent",
                        "filter callback artifact is not registered",
                    );
                    return finish_filter_callback_delivery_failure(
                        runtime,
                        handle,
                        CallbackDeliveryFailurePhase::CallbackArtifactLookup,
                        primary,
                    );
                }
                Err(error) => {
                    let primary = error.into_hal_error("IFilterCallback.onFilterEvent");
                    return finish_filter_callback_delivery_failure(
                        runtime,
                        handle,
                        CallbackDeliveryFailurePhase::CallbackArtifactLookup,
                        primary,
                    );
                }
            };
            let event = match event_from_snapshot(snapshot) {
                Ok(event) => event,
                Err(primary) => {
                    return finish_filter_callback_delivery_failure(
                        runtime,
                        handle,
                        CallbackDeliveryFailurePhase::EventConversion,
                        primary,
                    );
                }
            };
            if let Err(error) = callback.onFilterEvent(&[event]) {
                let primary = HalError::callback_failed(
                    "IFilterCallback.onFilterEvent",
                    format!("binder failure: {error:?}"),
                );
                return finish_filter_callback_delivery_failure(
                    runtime,
                    handle,
                    CallbackDeliveryFailurePhase::BinderDelivery,
                    primary,
                );
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use maleicacid_tuner_hal2_binder_adapter::{AidlObjectGeneration, AidlObjectId};
    use maleicacid_tuner_hal2_demux::av::AvMediaEventDescriptor;
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
