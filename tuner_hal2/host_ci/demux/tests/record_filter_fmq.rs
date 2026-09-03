use maleicacid_tuner_hal2_demux::{
    DemuxRuntime, DvrFilterLinkRequest, DvrKind, DvrRuntimeConfigureRequest,
    DvrRuntimeOperationRequest, DvrRuntimeRegistrationRequest, FilterConfig, FilterConfigKind,
    FilterOpenType, FilterRuntimeConfigureRequest, FilterRuntimeOperationRequest,
    FilterRuntimeRegistrationRequest, OpenFilterRequest, PipelineGeneratedEvent,
    QueueDescriptorQueryError, RecordIndexSettings, TsInputOrigin, ValidatedPacketIngressRequest,
    ValidatedTsPacket, DEMUX_TS_INDEX_PAYLOAD_UNIT_START, RECORD_SC_TYPE_NONE,
};
use maleicacid_tuner_hal2_fmq::{host_ci_queue_snapshots, host_ci_reset_queue_registry};

fn record_packet(pid: u16, continuity_counter: u8) -> [u8; 188] {
    let mut packet = [0xffu8; 188];
    packet[0] = 0x47;
    packet[1] = 0x40 | (((pid >> 8) as u8) & 0x1f);
    packet[2] = pid as u8;
    packet[3] = 0x10 | (continuity_counter & 0x0f);
    packet
}

fn configured_record_demux() -> DemuxRuntime {
    host_ci_reset_queue_registry();
    let mut demux = DemuxRuntime::new(1, 1);
    let request = OpenFilterRequest {
        open_type: FilterOpenType::TsRecord,
        buffer_size: 4096,
        callback_present: true,
    };
    demux
        .register_filter_from_typed_request(FilterRuntimeRegistrationRequest::new(1, &request, 64))
        .expect("record filter open must create its owned Filter FMQ");
    let (_, configured) = demux.configure_filter_runtime_with_typed_request(
        FilterRuntimeConfigureRequest::new(
            1,
            FilterConfig {
                open_type: FilterOpenType::TsRecord,
                tpid: 0x0100,
                kind: FilterConfigKind::TsRecord(RecordIndexSettings {
                    ts_index_mask: DEMUX_TS_INDEX_PAYLOAD_UNIT_START,
                    sc_index_type: RECORD_SC_TYPE_NONE,
                    sc_index_mask: 0,
                }),
            },
        ),
    );
    configured.expect("record filter configure must succeed");
    demux
        .start_filter_runtime_from_typed_request(FilterRuntimeOperationRequest::new(1))
        .expect("record filter start must succeed");

    demux
        .register_dvr_from_typed_request(DvrRuntimeRegistrationRequest::new(
            2,
            DvrKind::Record,
            8192,
            true,
        ))
        .expect("record DVR open must succeed");
    let (_, configured) =
        demux.configure_dvr_runtime_with_typed_request(DvrRuntimeConfigureRequest::new(2));
    configured.expect("record DVR configure must succeed");
    let prepared = demux
        .prepare_attach_dvr_filter_from_typed_request(DvrFilterLinkRequest::new(2, 1))
        .expect("record DVR attach prepare must succeed");
    demux
        .commit_prepared_dvr_filter_relation(prepared)
        .expect("record DVR attach commit must succeed");
    demux
        .start_dvr_runtime_from_typed_request(DvrRuntimeOperationRequest::new(2))
        .expect("record DVR start must succeed");
    demux
}

#[test]
fn record_filter_exports_standard_filter_fmq_descriptor() {
    assert!(FilterOpenType::TsRecord.has_filter_fmq());
    assert!(!FilterOpenType::TsRecord.uses_filter_fmq_for_payload());

    let demux = configured_record_demux();
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

#[test]
fn record_payload_stays_on_dvr_fmq_and_byte_number_follows_dvr_commits() {
    let mut demux = configured_record_demux();
    let first = record_packet(0x0100, 0);
    let first_validated = ValidatedTsPacket::validate(&first).expect("first packet must be valid");
    let first_report = demux.push_validated_ts_packet_from_typed_request(
        ValidatedPacketIngressRequest::new(&first_validated, TsInputOrigin::frontend(1)),
    );
    assert!(first_report.generated_events.iter().any(|event| matches!(
        event,
        PipelineGeneratedEvent::RecordIndex { filter_id: 1, data }
            if data.byte_number == 0
    )));
    assert!(first_report.generated_events.iter().all(|event| !matches!(
        event,
        PipelineGeneratedEvent::FilterStatus { filter_id: 1, .. }
    )));

    let second = record_packet(0x0100, 1);
    let second_validated = ValidatedTsPacket::validate(&second).expect("second packet must be valid");
    let second_report = demux.push_validated_ts_packet_from_typed_request(
        ValidatedPacketIngressRequest::new(&second_validated, TsInputOrigin::frontend(1)),
    );
    assert!(second_report.generated_events.iter().any(|event| matches!(
        event,
        PipelineGeneratedEvent::RecordIndex { filter_id: 1, data }
            if data.byte_number == 188
    )));
    assert!(second_report.generated_events.iter().all(|event| !matches!(
        event,
        PipelineGeneratedEvent::FilterStatus { filter_id: 1, .. }
    )));

    let queues = host_ci_queue_snapshots();
    assert_eq!(queues.len(), 2, "record filter and record DVR must each own one queue");
    assert!(queues[0].is_empty(), "record payload must not enter the Filter FMQ");
    assert_eq!(
        queues[1],
        [first.to_vec(), second.to_vec()].concat(),
        "record payload must be committed to the Record DVR FMQ"
    );
}
