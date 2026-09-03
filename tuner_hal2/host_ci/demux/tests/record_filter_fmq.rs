use maleicacid_tuner_hal2_demux::{
    DemuxRuntime, FilterOpenType, FilterRuntimeRegistrationRequest, OpenFilterRequest,
    QueueDescriptorQueryError,
};

#[test]
fn record_filter_exports_standard_filter_fmq_descriptor() {
    assert!(FilterOpenType::TsRecord.has_filter_fmq());
    assert!(!FilterOpenType::TsRecord.uses_filter_fmq_for_payload());

    let mut demux = DemuxRuntime::new(1, 1);
    let request = OpenFilterRequest {
        open_type: FilterOpenType::TsRecord,
        buffer_size: 4096,
        callback_present: true,
    };
    demux
        .register_filter_from_typed_request(FilterRuntimeRegistrationRequest::new(
            1, &request, 64,
        ))
        .expect("record filter open must create its owned Filter FMQ");

    let descriptor = demux
        .filter_queue_descriptor_export_plan(1)
        .and_then(|plan| {
            plan.export_descriptor()
                .map_err(QueueDescriptorQueryError::Runtime)
        })
        .expect("record filter getQueueDesc path must export a valid descriptor");
    let (grantors, fds, _ints, quantum, _flags) = descriptor.into_parts();
    assert!(!grantors.is_empty());
    assert!(!fds.is_empty());
    assert!(quantum > 0);
}
