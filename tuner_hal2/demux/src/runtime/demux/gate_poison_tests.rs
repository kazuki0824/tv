use super::*;
use crate::packet_pipeline::PacketPid;
use crate::runtime::queue_runtime::QueueRuntimeLockKind;

fn configured_filter() -> (DemuxRuntime, FilterProducerDrainGate) {
    let mut demux = DemuxRuntime::new(1, 1);
    let request = OpenFilterRequest {
        open_type: FilterOpenType::TsRaw,
        buffer_size: 4096,
        callback_present: true,
    };
    demux.register_filter(DemuxRuntime::open_filter_runtime_from_request(17, 1, &request, None)).unwrap();
    demux.configure_filter_runtime(17, FilterPipelineConfig {
        tpid: Some(0x123),
        raw: false,
        record_index: None,
    }).unwrap();
    let gate = demux.filter_producer_gates.get(&17).unwrap().clone();
    (demux, gate)
}

fn assert_poison(error: DemuxRuntimeError, producer_release: bool) {
    assert_eq!(error.id, Some(17));
    assert!(matches!(error.kind,
        DemuxRuntimeErrorKind::QueueRuntimeFailureWithContext(QueueRuntimeError {
            kind: QueueRuntimeErrorKind::GateLockPoisoned {
                lock: QueueRuntimeLockKind::FilterProducerDrainGateData,
                poison_count: 1,
                counter_saturated: false,
                producer_release: recorded,
                drain_rollback: false,
            }, ..
        }) if recorded == producer_release));
}

#[test]
fn dropped_producer_poison_reaches_demux_callers() {
    type Operation = fn(&mut DemuxRuntime) -> Result<(), DemuxRuntimeError>;
    let operations: [Operation; 5] = [
        |demux| demux.clear_existing_filter_queue(17),
        |demux| demux.prepare_filter_queue_cleanup(FilterRuntimeOperationRequest::new(17)).map(|_| ()),
        |demux| demux.prepare_stream_boundary(PipelineBoundaryReason::TuneStart).map(|_| ()),
        |demux| demux.remove_filter(17).map(|_| ()),
        |demux| demux.enqueue_filter_queue_payload(17, vec![0; TS_PACKET_SIZE]),
    ];
    for operation in operations {
        let (mut demux, gate) = configured_filter();
        let permit = gate.begin_producer().unwrap();
        gate.poison_data_lock_for_test();
        drop(permit);
        assert_poison(operation(&mut demux).unwrap_err(), true);
    }
}

#[test]
fn drain_poison_after_prepare_reaches_cleanup_callers() {
    for discard_events in [true, false] {
        let (mut demux, gate) = configured_filter();
        let mut plan = demux.prepare_filter_queue_cleanup(FilterRuntimeOperationRequest::new(17)).unwrap();
        gate.poison_data_lock_for_test();
        let error = if discard_events {
            demux.discard_filter_pending_events_for_queue_cleanup(&mut plan).unwrap_err()
        } else {
            demux.commit_filter_producer_drain_for_queue_cleanup(plan).map(|_| ()).unwrap_err()
        };
        assert_poison(error, false);
        assert_eq!(demux.state(), DemuxRuntimeState::Quarantined);
    }
}

#[test]
fn producer_poison_reaches_packet_delivery_diagnostic() {
    let (mut demux, gate) = configured_filter();
    demux.start_filter_runtime(17).unwrap();
    let permit = gate.begin_producer().unwrap();
    gate.poison_data_lock_for_test();
    drop(permit);
    let mut events = vec![PipelineGeneratedEvent::DataReady { filter_id: 17 }];
    let diagnostics = demux.commit_generated_filter_events(
        &[0; TS_PACKET_SIZE], &mut events, PacketPid::from_config_pid(ConfigInputPid::validate_tpid(0x123).unwrap()), TsInputOrigin::frontend(1),
    );
    assert!(events.is_empty());
    assert!(diagnostics.iter().any(|diagnostic| {
        if let PipelineDiagnostic::FilterQueuePayloadDeliveryFailure { filter_id: 17, error, .. } = diagnostic {
            assert_poison(*error, true);
            true
        } else {
            false
        }
    }));
}
